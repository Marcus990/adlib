//! `ls-icons <icons-dir> <kind> "<query>" …` — top candidates for logo / icon / flag queries, in the same format as
//! `assets-pipeline/icon_search.py` (for parity checks) plus the auto-pick.
use ls_search::icons::IconSearch;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() >= 4, "usage: ls-icons <icons-dir> <logo|icon|flag> \"query\" …");
    let t = std::time::Instant::now();
    let s = IconSearch::load(std::path::Path::new(&a[1]))?;
    eprintln!("loaded in {} ms", t.elapsed().as_millis());
    for q in &a[3..] {
        let t = std::time::Instant::now();
        let (pick, ranked) = s.pick(&a[2], q);
        println!("[{}] {q}   ({} µs) pick={:?}", a[2], t.elapsed().as_micros(), pick.map(|h| h.id));
        for h in ranked {
            println!("('{}', {}, '{}', '{}')", h.id, h.score, h.how, h.title);
        }
    }
    Ok(())
}
