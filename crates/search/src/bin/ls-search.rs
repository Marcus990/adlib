//! Standalone search check: `ls-search <model-dir> <index.json> [phrase ...]` — prints the best
//! image per phrase with its score and runner-up, plus per-query latency.
use ls_search::{Clip, Index, Searcher};
use std::path::Path;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let clip = Clip::load(Path::new(&a[1]))?;
    let mut s = Searcher::new(Index::load(Path::new(&a[2]))?);
    if let Ok(t) = std::env::var("TEMPLATE") { s.template = if t.is_empty() { None } else { Some(t) }; }
    let _ = clip.embed_text("warm up")?;
    for p in &a[3..] {
        let t = Instant::now();
        let (m, hits) = s.best_match(&clip, 0, std::slice::from_ref(p))?;
        let el = t.elapsed().as_secs_f64() * 1000.0;
        let h = &hits[0];
        println!(
            "{el:6.1} ms  {p:<34} → {:<26} {:.3}   (2nd: {:?})",
            m.map(|m| m.image_id).unwrap_or_default(),
            h.score,
            h.runner_up.as_ref().map(|(i, s)| format!("{i} {s:.3}"))
        );
    }
    Ok(())
}
