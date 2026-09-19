//! Offline indexer: `ls-index <model-dir> <library-dir> <out.json>`.
//! Embeds one image at a time on the CPU (memory-safe on the 8 GB Mac) plus each caption.
use ls_search::{library_files, Clip, Entry, Index, MODEL_NAME};
use std::path::Path;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() == 4, "usage: ls-index <model-dir> <library-dir> <out.json>");
    let (mdir, lib, out) = (Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]));
    let clip = Clip::load(mdir)?;
    let files = library_files(lib)?;
    let root = std::fs::canonicalize(lib)?;
    let t0 = Instant::now();
    let mut entries = vec![];
    for (i, (id, file, caption)) in files.iter().enumerate() {
        let t = Instant::now();
        let img = match clip.embed_image(&lib.join(file)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("skip {file}: {e:#}");
                continue;
            }
        };
        let cap = clip.embed_text(&format!("a photo of {caption}"))?;
        println!("[{}/{}] {id} ({} ms)", i + 1, files.len(), t.elapsed().as_millis());
        entries.push(Entry { id: id.clone(), file: file.clone(), caption: caption.clone(), img, cap });
    }
    let idx = Index { model: MODEL_NAME.into(), root: root.to_string_lossy().into(), entries };
    std::fs::write(out, serde_json::to_string(&idx)?)?;
    println!("indexed {} images in {:.1} s → {}", idx.entries.len(), t0.elapsed().as_secs_f32(), out.display());
    Ok(())
}
