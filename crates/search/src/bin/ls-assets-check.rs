//! Diagnostic: re-embed a card image with the full CLIP ViT-B/32 vision tower and compare with the
//! stored row. Cosine ≈ 1 → the card's embeddings match this model + preprocessing; much lower → the
//! card was built differently (different preprocessing/model), and text queries can't align with it.
//!   ls-assets-check <assets-dir> <clip-dir> [n]
use anyhow::{Error as E, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear_no_bias, Module, VarBuilder};
use candle_transformers::models::clip::{vision_model::ClipVisionTransformer, ClipConfig};
use ls_search::assets;
use std::path::Path;

const MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
const STD: [f32; 3] = [0.26862954, 0.26130258, 0.27577711];

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (dir, clip_dir) = (Path::new(&a[1]), Path::new(&a[2]));
    let n: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let dev = Device::Cpu;
    let cfg = ClipConfig::vit_base_patch32();
    let vb = VarBuilder::from_pth(clip_dir.join("pytorch_model.bin"), DType::F32, &dev).map_err(E::msg)?;
    let vision = ClipVisionTransformer::new(vb.pp("vision_model"), &cfg.vision_config)?;
    let proj = linear_no_bias(cfg.vision_config.embed_dim, cfg.vision_config.projection_dim, vb.pp("visual_projection"))?;
    let index = assets::load_index(dir)?;
    for e in index.entries.iter().filter(|e| !e.caption.is_empty()).take(n) {
        let path = Path::new(&index.root).join(&e.file);
        let img = image::ImageReader::open(&path)?.with_guessed_format()?.decode()?;
        let img = img.resize_to_fill(224, 224, image::imageops::FilterType::CatmullRom).to_rgb8();
        let data: Vec<f32> = img
            .pixels()
            .flat_map(|p| (0..3).map(move |c| (p[c] as f32 / 255.0 - MEAN[c]) / STD[c]))
            .collect();
        let t = Tensor::from_vec(data, (224, 224, 3), &dev)?.permute((2, 0, 1))?.unsqueeze(0)?;
        let v = vision.forward(&t)?.apply(&proj)?.flatten_all()?.to_vec1::<f32>()?;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mine: Vec<f32> = v.iter().map(|x| x / norm).collect();
        let cos: f32 = mine.iter().zip(&e.img).map(|(a, b)| a * b).sum();
        println!("{} ({}): cosine(mine, card) = {cos:.4}", e.id, e.caption);
    }
    Ok(())
}
