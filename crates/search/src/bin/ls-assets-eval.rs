//! Retrieval quality + TAU calibration on the asset card (handoff AS3/AS5).
//! Zero-shot: for N labelled photos, score them against every distinct label and check the true label
//! wins; then report the score distribution of right vs wrong matches, which is what TAU must separate.
//!   ls-assets-eval <assets-dir> <clip-text-dir> [n] [template]
use anyhow::Result;
use ls_search::{assets, Searcher, TextEncoder};
use std::path::Path;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.get(3).map(|x| x == "why").unwrap_or(false) {
        let clip = assets::ClipText::load(Path::new(&a[2]))?;
        let index = assets::load_index(Path::new(&a[1]))?;
        let s = Searcher::new(index);
        for (query, label) in [("a rose", "rose"), ("a red rose", "rose"), ("rose", "rose"), ("a dog", "dog"), ("a penguin", "penguin"), ("a flower", "flower"), ("a rose", "flower")] {
            let q = clip.embed_text(&s.query_text(query))?;
            let l = clip.embed_text(&s.query_text(label))?;
            let aff: f32 = q.iter().zip(&l).map(|(a, b)| a * b).sum();
            let best = s.index.entries.iter().filter(|e| e.caption == label)
                .map(|e| e.img.iter().zip(&q).map(|(a, b)| a * b).sum::<f32>())
                .fold(f32::MIN, f32::max);
            println!("  query {query:<12} vs label {label:<10} affinity {aff:.3}   best image score for that label {best:.3}");
        }
        return Ok(());
    }
    if a.get(3).map(|x| x == "gated").unwrap_or(false) {
        // What the pipeline would actually show, with the label gate on.
        let clip = assets::ClipText::load(Path::new(&a[2]))?;
        let index = assets::load_index(Path::new(&a[1]))?;
        let cache = Path::new(&a[2]).join("label-vectors.json");
        let t = std::time::Instant::now();
        let s = Searcher::new(index).with_label_gate(&clip, &cache, 0.92, 0.31)?;
        println!("label gate ready in {:?}", t.elapsed());
        for qq in ["penguin", "a dog", "a rose", "a guitar", "a laptop", "a microphone", "an eagle",
                   "an owl", "a sunflower", "the planet earth", "quantum chromodynamics", "the concept of teamwork",
                   "our deployment pipeline", "a bicycle", "pizza"] {
            let t = std::time::Instant::now();
            let q = clip.embed_text(&s.query_text(qq))?;
            match s.hit_for(qq, &q) {
                Some(h) => println!("  {qq:<26} → {:<14} score {:.3}  ({:?})", h.caption, h.score, t.elapsed()),
                None => println!("  {qq:<26} → (nothing shown)      ({:?})", t.elapsed()),
            }
        }
        return Ok(());
    }
    if a.get(3).map(|x| x == "labels").unwrap_or(false) {
        // Does the top hit's own class label agree with the query? Image score alone cannot tell a real
        // match from "nothing fits" in a 15k generic library; label affinity might.
        let clip = assets::ClipText::load(Path::new(&a[2]))?;
        let index = assets::load_index(Path::new(&a[1]))?;
        let s = Searcher::new(index);
        let queries = ["penguin", "a dog", "pizza", "a bicycle", "a cat", "a guitar", "an owl", "a sunflower",
                       "quantum chromodynamics", "the concept of teamwork", "our deployment pipeline", "latency budget"];
        for qq in queries {
            let q = clip.embed_text(&s.query_text(qq))?;
            let v = s.score_vec(&q);
            let mut idx: Vec<usize> = (0..v.len()).collect();
            idx.sort_by(|x, y| v[*y].partial_cmp(&v[*x]).unwrap());
            let top = idx[0];
            let label = s.index.entries[top].caption.clone();
            let aff = if label.is_empty() { f32::NAN } else {
                let lt = clip.embed_text(&s.query_text(&label))?;
                q.iter().zip(&lt).map(|(p, r)| p * r).sum::<f32>()
            };
            // best hit among photos whose label is close to the query
            let mut best_lbl = (String::new(), 0.0f32, 0.0f32);
            for i in idx.iter().take(200) {
                let e = &s.index.entries[*i];
                if e.caption.is_empty() { continue }
                let lt = clip.embed_text(&s.query_text(&e.caption))?;
                let aff2: f32 = q.iter().zip(&lt).map(|(p, r)| p * r).sum();
                if aff2 > best_lbl.2 { best_lbl = (e.caption.clone(), v[*i], aff2); }
            }
            println!("{qq:<26} top {:.3} label {label:<14} affinity {aff:.3} | best-labelled: {:<14} img {:.3} aff {:.3}", v[top], best_lbl.0, best_lbl.1, best_lbl.2);
        }
        return Ok(());
    }
    if a.get(3).map(|x| x == "text").unwrap_or(false) {
        // Text-space sanity: related phrases should be far closer than unrelated ones.
        let clip = assets::ClipText::load(Path::new(&a[2]))?;
        let pairs = [("a photo of a dog", "a photo of a puppy"), ("a photo of a dog", "a photo of a bulldozer"),
                     ("a photo of an owl", "a photo of a bird"), ("a photo of an owl", "a photo of a pizza"),
                     ("a photo of a guitar", "a photo of a violin"), ("a photo of a guitar", "a photo of a mountain")];
        for (x, y) in pairs {
            let (a1, b1) = (clip.embed_text(x)?, clip.embed_text(y)?);
            let c: f32 = a1.iter().zip(&b1).map(|(p, q)| p * q).sum();
            println!("  cos({x:?}, {y:?}) = {c:.3}");
        }
        return Ok(());
    }
    let n: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(300);
    let template = a.get(4).cloned().unwrap_or_else(|| "a photo of {}".into());
    let clip = assets::ClipText::load(Path::new(&a[2]))?;
    let index = assets::load_index(Path::new(&a[1]))?;
    let labels = index.vocab(1000);
    println!("{} photos, {} distinct labels, template {template:?}", index.entries.len(), labels.len());
    let texts: Vec<Vec<f32>> = labels
        .iter()
        .map(|l| clip.embed_text(&template.replace("{}", l)))
        .collect::<Result<_>>()?;
    let (mut right, mut wrong, mut hit_scores, mut miss_scores) = (0, 0, vec![], vec![]);
    let step = (index.entries.len() / n).max(1);
    for e in index.entries.iter().step_by(step).filter(|e| !e.caption.is_empty()).take(n) {
        let mut best = (0usize, f32::MIN);
        for (i, t) in texts.iter().enumerate() {
            let s: f32 = t.iter().zip(&e.img).map(|(a, b)| a * b).sum();
            if s > best.1 {
                best = (i, s);
            }
        }
        if labels[best.0] == e.caption {
            right += 1;
            hit_scores.push(best.1);
        } else {
            wrong += 1;
            miss_scores.push(best.1);
        }
    }
    let pct = |v: &mut Vec<f32>, q: f32| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.get(((v.len() as f32 * q) as usize).min(v.len().saturating_sub(1))).copied().unwrap_or(0.0)
    };
    println!("top-1 label correct: {right}/{} ({:.0}%)", right + wrong, 100.0 * right as f32 / (right + wrong) as f32);
    println!("correct-match scores  p10 {:.3}  p50 {:.3}  p90 {:.3}", pct(&mut hit_scores, 0.1), pct(&mut hit_scores, 0.5), pct(&mut hit_scores, 0.9));
    println!("wrong-match scores    p10 {:.3}  p50 {:.3}  p90 {:.3}", pct(&mut miss_scores, 0.1), pct(&mut miss_scores, 0.5), pct(&mut miss_scores, 0.9));
    // what a real query looks like: best score for phrases with and without a match in the library
    let s = Searcher::new(index);
    for phrase in ["penguin", "a dog", "pizza", "guitar", "an owl", "a sunflower", "planet earth from space", "quantum chromodynamics", "the concept of teamwork"] {
        let q = clip.embed_text(&template.replace("{}", phrase))?;
        let v = s.score_vec(&q);
        let (mut bi, mut bs) = (0, f32::MIN);
        for (i, x) in v.iter().enumerate() {
            if *x > bs {
                bs = *x;
                bi = i;
            }
        }
        // Absolute cosines barely separate a real hit from "nothing fits"; the margin over the library's
        // own score distribution does. z = (best - mean) / sd over all 15k photos for this query.
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let sd = (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / v.len() as f32).sqrt();
        let mut sorted = v.clone();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
        println!("  {phrase:<28} best {bs:.3}  z {:.1}  2nd {:.3}  mean {mean:.3}  {:<14} {}", (bs - mean) / sd, sorted[1], s.index.entries[bi].caption, s.index.entries[bi].file);
    }
    Ok(())
}

// appended: text-space sanity (run with argv[3] = "text")
