//! Does Whisper's own confidence separate a real subject from a mis-hearing?
//!   ls-probs <whisper-model> <vad-model> <wav> [words…]
//! Prints, for every finished phrase, its no-speech probability and the probability of each word,
//! flagging the words asked about on the command line.
use anyhow::Result;
use ls_hear::{whisper, Asr, SR};
use std::path::Path;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let watch: Vec<String> = a[4..].iter().map(|w| w.to_lowercase()).collect();
    let mut asr = whisper::WhisperAsr::load(&a[1])?;
    let pcm = ls_hear::audio::load_wav(Path::new(&a[3]))?;
    let _ = asr.transcribe(&vec![0.0; SR]);
    // Whole-phrase windows: 6 s hops, which is roughly how long a spoken sentence runs.
    let win = SR * 6;
    for (i, block) in pcm.chunks(win).enumerate() {
        let h = asr.transcribe_detailed(block)?;
        let text = h.text.trim();
        if text.is_empty() {
            continue;
        }
        let ps: Vec<f32> = h.tokens.iter().map(|(_, p)| *p).collect();
        let mean = ps.iter().sum::<f32>() / ps.len().max(1) as f32;
        let min = ps.iter().cloned().fold(1.0f32, f32::min);
        println!("\n[{:>5.1}s] no_speech {:.2}  mean_p {:.2}  min_p {:.2}  {text:?}", (i * win) as f32 / SR as f32, h.no_speech, mean, min);
        let mut line = String::new();
        for (t, p) in &h.tokens {
            let w = t.trim().to_lowercase();
            let mark = if watch.iter().any(|x| w.contains(x.as_str())) { "*" } else { "" };
            line.push_str(&format!("{mark}{}({:.2}) ", t.trim(), p));
        }
        println!("   {line}");
    }
    Ok(())
}
