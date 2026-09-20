//! Add locally supplied JPEGs to an OpenAI CLIP ViT-B/32 asset card.
//! Usage: ls-assets-add <assets-dir> <clip-dir> <jpeg> <label> [<jpeg> <label> ...]
//!        ls-assets-add <assets-dir> <clip-dir> --replace <id> <jpeg> [<id> <jpeg> ...]
//!
//! Source files must already be JPEGs. This keeps HEIC conversion outside the card and preserves
//! the card convention of a compact, self-contained `images/` directory.
use anyhow::{Context, Error as E, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear_no_bias, Module, VarBuilder};
use candle_transformers::models::clip::{vision_model::ClipVisionTransformer, ClipConfig};
use ls_search::{assets, DIM};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
const STD: [f32; 3] = [0.26862954, 0.26130258, 0.27577711];

fn embed(path: &Path, vision: &ClipVisionTransformer, proj: &candle_nn::Linear, dev: &Device) -> Result<Vec<f32>> {
    let img = image::ImageReader::open(path)?.with_guessed_format()?.decode()?;
    let img = img.resize_to_fill(224, 224, image::imageops::FilterType::CatmullRom).to_rgb8();
    let data: Vec<f32> = img
        .pixels()
        .flat_map(|p| (0..3).map(move |c| (p[c] as f32 / 255.0 - MEAN[c]) / STD[c]))
        .collect();
    Ok(vision.forward(&Tensor::from_vec(data, (224, 224, 3), dev)?.permute((2, 0, 1))?.unsqueeze(0)?)?
        .apply(proj)?
        .flatten_all()?
        .to_vec1::<f32>()?)
}

fn write_npy(path: &Path, rows: &[Vec<f32>]) -> Result<()> {
    anyhow::ensure!(!rows.is_empty() && rows.iter().all(|r| r.len() == DIM), "expected non-empty {DIM}-d rows");
    let (n, dim) = (rows.len(), rows[0].len());
    let mut header = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({n}, {dim}), }}");
    while (10 + header.len() + 1) % 64 != 0 { header.push(' '); }
    header.push('\n');
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend((header.len() as u16).to_le_bytes());
    out.extend(header.as_bytes());
    for row in rows { for value in row { out.extend(value.to_le_bytes()); } }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let replacing = a.get(3).is_some_and(|arg| arg == "--replace");
    let first_pair = if replacing { 4 } else { 3 };
    if a.len() < first_pair + 2 || (a.len() - first_pair) % 2 != 0 {
        anyhow::bail!("usage: ls-assets-add <assets-dir> <clip-dir> <jpeg> <label> [<jpeg> <label> ...]\n       ls-assets-add <assets-dir> <clip-dir> --replace <id> <jpeg> [<id> <jpeg> ...]");
    }
    let (dir, clip_dir) = (Path::new(&a[1]), Path::new(&a[2]));
    let manifest_path = dir.join("manifest.json");
    let mut manifest: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    let (mut rows, dim) = assets::read_npy(&dir.join("embeddings.npy"))?;
    anyhow::ensure!(dim == DIM && rows.len() == manifest.len(), "asset card is not index-aligned");
    let start = manifest.iter().filter_map(|e| e.get("id")?.as_str()?.parse::<u32>().ok()).max().unwrap_or(0) + 1;

    let dev = Device::Cpu;
    let cfg = ClipConfig::vit_base_patch32();
    let vb = VarBuilder::from_pth(clip_dir.join("pytorch_model.bin"), DType::F32, &dev).map_err(E::msg)?;
    let vision = ClipVisionTransformer::new(vb.pp("vision_model"), &cfg.vision_config)?;
    let proj = linear_no_bias(cfg.vision_config.embed_dim, cfg.vision_config.projection_dim, vb.pp("visual_projection"))?;

    let images = dir.join("images");
    let mut pending: Vec<(PathBuf, PathBuf)> = vec![];
    for (offset, pair) in a[first_pair..].chunks_exact(2).enumerate() {
        let (id, source, dest) = if replacing {
            let id = &pair[0];
            let item = manifest.iter().position(|e| e.get("id").and_then(Value::as_str) == Some(id))
                .with_context(|| format!("no asset-card entry {id}"))?;
            let filename = manifest[item].get("filename").and_then(Value::as_str).context("manifest entry has no filename")?;
            (id.clone(), Path::new(&pair[1]), images.join(filename))
        } else {
            let id = format!("{:06}", start + offset as u32);
            let filename = format!("{id}.jpg");
            let dest = images.join(&filename);
            anyhow::ensure!(!dest.exists(), "{} already exists", dest.display());
            (id, Path::new(&pair[0]), dest)
        };
        anyhow::ensure!(source.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")), "{} is not a JPEG", source.display());
        let embedding = embed(source, &vision, &proj, &dev)?;
        if replacing {
            let item = manifest.iter().position(|e| e.get("id").and_then(Value::as_str) == Some(&id)).unwrap();
            rows[item] = embedding;
        } else {
            let filename = dest.file_name().and_then(|x| x.to_str()).context("destination has no filename")?;
            rows.push(embedding);
            manifest.push(json!({"id": id, "filename": filename, "source_dataset": "local", "class_label": pair[1]}));
        }
        pending.push((source.to_path_buf(), dest));
    }

    let npy_next = dir.join("embeddings.npy.next");
    let manifest_next = dir.join("manifest.json.next");
    write_npy(&npy_next, &rows)?;
    std::fs::write(&manifest_next, serde_json::to_vec_pretty(&manifest)?)?;
    for (source, dest) in &pending { std::fs::copy(source, dest).with_context(|| format!("copying {}", source.display()))?; }
    std::fs::rename(&npy_next, dir.join("embeddings.npy"))?;
    std::fs::rename(&manifest_next, &manifest_path)?;
    println!("{} {} photo(s); library now has {} rows.", if replacing { "Replaced" } else { "Added" }, pending.len(), rows.len());
    for (_, dest) in pending { println!("  {}", dest.display()); }
    Ok(())
}
