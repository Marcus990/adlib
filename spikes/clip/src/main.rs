//! Spike S4: Candle MobileCLIP (v1 S2) on Metal — load, embed images, embed text, rank.
//! Usage: spike-clip <model-dir> <library-dir> [queries...]
use anyhow::{Error as E, Result};
use candle_core::{DType, Device, Tensor, D};
use candle_nn::VarBuilder;
use candle_transformers::models::mobileclip;
use std::time::Instant;
use tokenizers::Tokenizer;

fn load_image(path: &std::path::Path, size: usize, dev: &Device) -> Result<Tensor> {
    let img = image::ImageReader::open(path)?.decode()?;
    let img = img.resize_to_fill(size as u32, size as u32, image::imageops::FilterType::Triangle);
    let data = img.to_rgb8().into_raw();
    let t = Tensor::from_vec(data, (size, size, 3), dev)?.permute((2, 0, 1))?;
    Ok((t.to_dtype(DType::F32)? / 255.0)?)
}

fn l2(v: &Tensor) -> Result<Tensor> {
    Ok(v.broadcast_div(&v.sqr()?.sum_keepdim(D::Minus1)?.sqrt()?)?)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (mdir, lib) = (std::path::Path::new(&args[1]), std::path::Path::new(&args[2]));
    let queries: Vec<String> = if args.len() > 3 {
        args[3..].to_vec()
    } else {
        ["a bird of prey", "tokyo office skyline", "someone playing music", "a red flower", "a ball game", "the planet earth from space", "a thunderstorm"]
            .iter().map(|s| s.to_string()).collect()
    };
    let dev = if std::env::var("CPU").is_ok() { Device::Cpu } else { Device::new_metal(0)? };
    let limit: usize = std::env::var("LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let cfg = mobileclip::MobileClipConfig::s2();

    let t = Instant::now();
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[mdir.join("open_clip_model.safetensors")], DType::F32, &dev)?
    };
    let model = mobileclip::MobileClipModel::new(vb, &cfg)?;
    let tok = Tokenizer::from_file(mdir.join("tokenizer.json")).map_err(E::msg)?;
    println!("load: {} ms", t.elapsed().as_millis());

    let mut paths: Vec<_> = std::fs::read_dir(lib)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "jpg").unwrap_or(false))
        .collect();
    paths.sort();
    paths.truncate(limit);

    let t = Instant::now();
    let mut embs = vec![];
    for chunk in paths.chunks(8) {
        let imgs: Vec<Tensor> = chunk.iter().map(|p| load_image(p, cfg.image_size, &dev)).collect::<Result<_>>()?;
        let batch = Tensor::stack(&imgs, 0)?;
        let tb = Instant::now();
        let e = l2(&model.get_image_features(&batch)?)?;
        let _ = e.flatten_all()?.to_vec1::<f32>()?; // force sync
        println!("  batch of {}: {} ms", chunk.len(), tb.elapsed().as_millis());
        embs.push(e);
    }
    let img_emb = Tensor::cat(&embs, 0)?;
    println!("image embed {} images: {} ms  dim={:?}", paths.len(), t.elapsed().as_millis(), img_emb.dims());

    // warm-up text
    let _ = embed_text(&model, &tok, "warm up", &dev)?;
    for q in &queries {
        let t = Instant::now();
        let te = embed_text(&model, &tok, q, &dev)?;
        let sims = img_emb.matmul(&te.t()?)?.flatten_all()?.to_vec1::<f32>()?;
        let el = t.elapsed().as_micros();
        let mut idx: Vec<usize> = (0..sims.len()).collect();
        idx.sort_by(|a, b| sims[*b].partial_cmp(&sims[*a]).unwrap());
        let top: Vec<String> = idx[..3].iter()
            .map(|i| format!("{} {:.3}", paths[*i].file_stem().unwrap().to_string_lossy(), sims[*i]))
            .collect();
        println!("{:>6.1} ms  {:<32} → {}", el as f64 / 1000.0, q, top.join(" | "));
    }
    Ok(())
}

fn embed_text(model: &mobileclip::MobileClipModel, tok: &Tokenizer, s: &str, dev: &Device) -> Result<Tensor> {
    let ids = tok.encode(s, true).map_err(E::msg)?.get_ids().to_vec();
    let input = Tensor::new(vec![ids], dev)?;
    l2(&model.get_text_features(&input)?)
}
