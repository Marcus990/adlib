//! Marcus's asset library: ~15k photos on the SD card whose image embeddings
//! were precomputed with **OpenAI CLIP ViT-B/32**. Queries must use that model's *text* tower — MobileCLIP
//! vectors live in a different space and cannot be compared with these, even though both are 512-d.
//!
//! Only the text tower is loaded. On first run the PyTorch checkpoint is converted once into a
//! text-only safetensors file (~250 MB instead of 605 MB), which then mmaps in milliseconds.

use anyhow::{Context, Error as E, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear_no_bias, Linear, Module, VarBuilder};
use candle_transformers::models::clip::{text_model::ClipTextTransformer, ClipConfig};
use serde::Deserialize;
use std::path::Path;
use tokenizers::Tokenizer;

use crate::{Entry, Index, TextEncoder, DIM};

/// `Index.model` for a library built from the asset card.
pub const ASSETS_MODEL: &str = "clip-vit-b32";
/// Text-tower weights, extracted from `pytorch_model.bin` on first load.
pub const TEXT_WEIGHTS: &str = "clip-text-vit-b32.safetensors";

pub struct ClipText {
    text: ClipTextTransformer,
    proj: Linear,
    tok: Tokenizer,
    dev: Device,
}

impl ClipText {
    /// `dir` holds `tokenizer.json` and either `clip-text-vit-b32.safetensors` (preferred) or the
    /// `pytorch_model.bin` of openai/clip-vit-base-patch32, which is converted once.
    pub fn load(dir: &Path) -> Result<Self> {
        let weights = dir.join(TEXT_WEIGHTS);
        if !weights.exists() {
            let bin = dir.join("pytorch_model.bin");
            anyhow::ensure!(bin.exists(), "no {TEXT_WEIGHTS} and no pytorch_model.bin in {}", dir.display());
            extract_text_tower(&bin, &weights).with_context(|| format!("converting {}", bin.display()))?;
        }
        let dev = Device::Cpu;
        let cfg = ClipConfig::vit_base_patch32();
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[&weights], DType::F32, &dev)? };
        let text = ClipTextTransformer::new(vb.pp("text_model"), &cfg.text_config)?;
        let proj = linear_no_bias(cfg.text_config.embed_dim, cfg.text_config.projection_dim, vb.pp("text_projection"))?;
        let tok = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(E::msg)?;
        Ok(Self { text, proj, tok, dev })
    }
}

impl TextEncoder for ClipText {
    fn embed_text(&self, s: &str) -> Result<Vec<f32>> {
        let mut ids = self.tok.encode(s, true).map_err(E::msg)?.get_ids().to_vec();
        ids.truncate(77); // CLIP context length; pooling takes the end-of-text token (the largest id)
        let input = Tensor::new(vec![ids], &self.dev)?;
        let v = self.text.forward(&input)?.apply(&self.proj)?;
        let v = crate::l2(&v)?;
        Ok(v.flatten_all()?.to_vec1::<f32>()?)
    }
    fn model_name(&self) -> &'static str {
        ASSETS_MODEL
    }
}

/// Keep `text_model.*` and `text_projection.weight`; drop the vision tower (~350 MB).
fn extract_text_tower(bin: &Path, out: &Path) -> Result<()> {
    let all = candle_core::pickle::read_all(bin)?;
    let keep: std::collections::HashMap<String, Tensor> = all
        .into_iter()
        .filter(|(k, _)| k.starts_with("text_model.") || k == "text_projection.weight")
        .collect();
    anyhow::ensure!(!keep.is_empty(), "no text tower in {} (keys unexpected)", bin.display());
    candle_core::safetensors::save(&keep, out)?;
    Ok(())
}

#[derive(Deserialize)]
struct ManifestEntry {
    id: serde_json::Value, // ids are strings in the card's manifest, but tolerate numbers
    filename: String,
    #[serde(default)]
    class_label: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Manifest {
    List(Vec<ManifestEntry>),
    Wrapped { entries: Vec<ManifestEntry> },
}

/// Build an in-memory [`Index`] from the card: `manifest.json` + `embeddings.npy` (rows are NOT
/// L2-normalized on disk — norms ≈ 10–11.6 — so they are normalized here).
pub fn load_index(dir: &Path) -> Result<Index> {
    let raw = std::fs::read_to_string(dir.join("manifest.json"))
        .with_context(|| format!("reading {}/manifest.json (is the asset card mounted?)", dir.display()))?;
    let items = match serde_json::from_str::<Manifest>(&raw)? {
        Manifest::List(v) => v,
        Manifest::Wrapped { entries } => entries,
    };
    let (rows, dim) = read_npy(&dir.join("embeddings.npy"))?;
    anyhow::ensure!(dim == DIM, "embeddings are {dim}-d, expected {DIM}");
    anyhow::ensure!(rows.len() == items.len(), "manifest has {} entries but embeddings.npy has {} rows", items.len(), rows.len());
    let entries = items
        .into_iter()
        .zip(rows)
        .map(|(m, img)| Entry {
            id: match m.id {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            },
            file: format!("images/{}", m.filename),
            caption: m.class_label.unwrap_or_default().to_lowercase(),
            img: normalize(img),
            cap: vec![],
        })
        .collect();
    Ok(Index { model: ASSETS_MODEL.into(), root: dir.display().to_string(), entries })
}

fn normalize(v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 { v.into_iter().map(|x| x / n).collect() } else { v }
}

/// Minimal `.npy` v1/v2 reader for a C-order little-endian float32 matrix.
pub fn read_npy(path: &Path) -> Result<(Vec<Vec<f32>>, usize)> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(bytes.len() > 12 && &bytes[..6] == b"\x93NUMPY", "{} is not a .npy file", path.display());
    let (header_len, start) = match bytes[6] {
        1 => (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10),
        2.. => (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12),
        v => anyhow::bail!("unsupported .npy version {v}"),
    };
    let header = std::str::from_utf8(&bytes[start..start + header_len])?;
    anyhow::ensure!(header.contains("'<f4'") || header.contains("\"<f4\""), "expected float32 rows in {header}");
    anyhow::ensure!(header.contains("'fortran_order': False"), "expected C order in {header}");
    let shape = header
        .split("'shape':")
        .nth(1)
        .and_then(|s| s.split('(').nth(1))
        .and_then(|s| s.split(')').next())
        .context("no shape in .npy header")?;
    let dims: Vec<usize> = shape.split(',').filter_map(|d| d.trim().parse().ok()).collect();
    anyhow::ensure!(dims.len() == 2, "expected a 2-D array, got {shape}");
    let (n, dim) = (dims[0], dims[1]);
    let data = &bytes[start + header_len..];
    anyhow::ensure!(data.len() == n * dim * 4, "{} has {} data bytes, expected {}", path.display(), data.len(), n * dim * 4);
    let rows = data
        .chunks_exact(dim * 4)
        .map(|row| row.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
        .collect();
    Ok((rows, dim))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_npy(path: &Path, rows: &[Vec<f32>]) {
        let (n, dim) = (rows.len(), rows[0].len());
        let mut header = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({n}, {dim}), }}");
        while (10 + header.len() + 1) % 64 != 0 {
            header.push(' ');
        }
        header.push('\n');
        let mut out = b"\x93NUMPY\x01\x00".to_vec();
        out.extend((header.len() as u16).to_le_bytes());
        out.extend(header.as_bytes());
        for r in rows {
            for x in r {
                out.extend(x.to_le_bytes());
            }
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn reads_a_card_shaped_library() {
        let dir = std::env::temp_dir().join(format!("ls-assets-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rows: Vec<Vec<f32>> = vec![
            (0..DIM).map(|i| if i == 0 { 10.0 } else { 0.0 }).collect(), // unnormalized, like the card
            (0..DIM).map(|i| if i == 1 { -11.6 } else { 0.0 }).collect(),
        ];
        write_npy(&dir.join("embeddings.npy"), &rows);
        std::fs::write(
            dir.join("manifest.json"),
            r#"[{"id":"000001","filename":"000001.jpg","source_dataset":"openimages","class_label":"Dog"},
                {"id":"000002","filename":"000002.jpg","source_dataset":"coco"}]"#,
        )
        .unwrap();
        let idx = load_index(&dir).unwrap();
        assert_eq!(idx.model, ASSETS_MODEL);
        assert_eq!(idx.entries.len(), 2);
        assert_eq!(idx.entries[0].file, "images/000001.jpg");
        assert_eq!(idx.entries[0].caption, "dog");
        assert_eq!(idx.entries[1].caption, "", "COCO rows have no label; the embedding is the only signal");
        let n: f32 = idx.entries[0].img.iter().map(|x| x * x).sum();
        assert!((n - 1.0).abs() < 1e-5, "rows are normalized on load, got norm² {n}");
        assert!(idx.entries[1].img[1] < 0.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
