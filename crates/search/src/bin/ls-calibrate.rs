//! Calibrate retrieval on a labelled query set (design doc §7.3 step 4).
//!   ls-calibrate <model-dir> <index.json> <labels.tsv>
//! labels.tsv: `phrase<TAB>expected_image_id` ("-" = nothing in the library should match).
//! Reports top-1 accuracy with and without the CLIP template, the score ranges of correct vs wrong/no-match
//! bests, and the τ that maximises correct accept/reject decisions (ties → midpoint of the gap).
use ls_search::{Clip, Index, Searcher};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() == 4, "usage: ls-calibrate <model-dir> <index.json> <labels.tsv>");
    let clip = Clip::load(Path::new(&a[1]))?;
    let mut s = Searcher::new(Index::load(Path::new(&a[2]))?);
    let labels: Vec<(String, String)> = std::fs::read_to_string(&a[3])?
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t').map(|(p, e)| (p.trim().to_string(), e.trim().to_string())))
        .collect();
    anyhow::ensure!(!labels.is_empty(), "no labels");
    for template in [Some("a photo of {}".to_string()), None] {
        s.template = template.clone();
        // (score, should_accept) for each query's best hit.
        let mut pts: Vec<(f32, bool)> = vec![];
        let (mut top1, mut positives) = (0, 0);
        println!("\n== template {:?}", template);
        for (phrase, want) in &labels {
            let q = clip.embed_text(&s.query_text(phrase))?;
            let h = s.hit_for(phrase, &q).unwrap();
            let is_pos = want != "-";
            let right = is_pos && &h.id == want;
            positives += is_pos as usize;
            top1 += right as usize;
            pts.push((h.score, right));
            println!("{} {:<34} → {:<26} {:.3}  (want {want})", if right || !is_pos { " " } else { "✗" }, phrase, h.id, h.score);
        }
        let ok: Vec<f32> = pts.iter().filter(|p| p.1).map(|p| p.0).collect();
        let bad: Vec<f32> = pts.iter().filter(|p| !p.1).map(|p| p.0).collect();
        // Best τ: accept iff score ≥ τ; count correct decisions.
        let mut cands: Vec<f32> = pts.iter().map(|p| p.0).collect();
        cands.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let mut best = (0usize, 0.0f32);
        for (i, c) in cands.iter().enumerate() {
            let tau = if i == 0 { c - 0.001 } else { (c + cands[i - 1]) / 2.0 };
            let correct = pts.iter().filter(|(sc, good)| (*sc >= tau) == *good).count();
            if correct > best.0 {
                best = (correct, tau);
            }
        }
        let fmt = |v: &[f32]| {
            if v.is_empty() { "—".into() } else {
                format!("{:.3}–{:.3}", v.iter().cloned().fold(f32::MAX, f32::min), v.iter().cloned().fold(f32::MIN, f32::max))
            }
        };
        println!(
            "top-1 {top1}/{positives}; correct bests {}; wrong/no-match bests {}; best τ = {:.3} ({}/{} accept/reject right)",
            fmt(&ok),
            fmt(&bad),
            best.1,
            best.0,
            pts.len()
        );
    }
    Ok(())
}
