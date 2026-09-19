//! Query Marcus's asset card from the CLI — for calibration (handoff AS3/AS5) and spot checks.
//!   ls-assets <assets-dir> <clip-text-dir> "a phrase" ["another"]
//! Prints the top 5 photos per phrase with cosine scores, plus timings.
use anyhow::Result;
use ls_search::{assets, Searcher, TextEncoder};
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: ls-assets <assets-dir> <clip-text-dir> <phrase> [phrase…]");
        std::process::exit(2);
    }
    let t = Instant::now();
    let clip = assets::ClipText::load(Path::new(&a[2]))?;
    let load_model = t.elapsed();
    let t = Instant::now();
    let index = assets::load_index(Path::new(&a[1]))?;
    let (n, load_index_ms) = (index.entries.len(), t.elapsed());
    let labelled = index.entries.iter().filter(|e| !e.caption.is_empty()).count();
    println!("model {:?}  index {n} photos ({labelled} labelled) in {load_index_ms:?}  (model load {load_model:?})", load_model);
    let s = Searcher::new(index);
    let _ = clip.embed_text("warm up");
    for phrase in &a[3..] {
        let t = Instant::now();
        let q = clip.embed_text(&s.query_text(phrase))?;
        let embed = t.elapsed();
        let t = Instant::now();
        let scores = s.score_vec(&q);
        let search = t.elapsed();
        let mut idx: Vec<usize> = (0..scores.len()).collect();
        idx.sort_by(|x, y| scores[*y].partial_cmp(&scores[*x]).unwrap());
        println!("\n{phrase:?}  (embed {embed:?}, search {search:?})");
        for i in idx.into_iter().take(5) {
            let e = &s.index.entries[i];
            println!("   {:.4}  {:<10} {:<22} {}", scores[i], e.id, e.caption, e.file);
        }
    }
    Ok(())
}
