//! Standalone Hear check. Prints one JSON line per chunk event with ASR timing.
//!   ls-hear <whisper-model> <vad-model> --wav file.wav [--realtime]
//!   ls-hear <whisper-model> <vad-model> --mic [device-substring] [--seconds N]
//!   ls-hear --list
use ls_hear::{audio, whisper, Chunker, ChunkerConfig, SR};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(|s| s.as_str()) == Some("--probe") {
        // Capture only: prove audio blocks arrive, with level. `ls-hear --probe [device] [secs]`
        let hint = a.get(2).map(|s| s.as_str()).filter(|s| *s != "default");
        let secs: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
        let (_s, rx, name) = audio::capture(hint)?;
        let t0 = Instant::now();
        let (mut n, mut samples, mut peak) = (0usize, 0usize, 0f32);
        while t0.elapsed() < Duration::from_secs(secs) {
            if let Ok(b) = rx.recv_timeout(Duration::from_millis(200)) {
                n += 1;
                samples += b.len();
                peak = b.iter().fold(peak, |m, x| m.max(x.abs()));
            }
        }
        println!("device={name:?} blocks={n} seconds_of_audio={:.2} peak={peak:.4}", samples as f32 / SR as f32);
        return Ok(());
    }
    if a.get(1).map(|s| s.as_str()) == Some("--list") {
        for d in audio::list_inputs() {
            println!("{d}");
        }
        return Ok(());
    }
    let terms: Vec<String> = std::env::var("TALK_TERMS").unwrap_or_default().split([',', '\n']).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    let asr = whisper::WhisperAsr::load(&a[1])?.with_vocabulary(&terms);
    let vad = whisper::SileroVad::load(&a[2])?;
    let mut ch = Chunker::new(ChunkerConfig::default(), asr, vad);
    let print = |c: &ls_contracts::Chunk, t: &ls_hear::ChunkTiming| {
        println!(
            "{}",
            serde_json::json!({"chunk": c, "audio_ms": t.audio_ms, "vad_ms": t.vad_ms.round(), "asr_ms": t.asr_ms.round()})
        );
    };
    match a.get(3).map(|s| s.as_str()) {
        Some("--wav") => {
            let pcm = audio::load_wav(std::path::Path::new(&a[4]))?;
            let realtime = a.iter().any(|x| x == "--realtime");
            let t0 = Instant::now();
            for (i, block) in pcm.chunks(SR / 10).enumerate() {
                if realtime {
                    let due = Duration::from_millis(i as u64 * 100);
                    if let Some(w) = due.checked_sub(t0.elapsed()) {
                        std::thread::sleep(w);
                    }
                }
                for (c, t) in ch.push(block)? {
                    print(&c, &t);
                }
            }
            for (c, t) in ch.finish()? {
                print(&c, &t);
            }
        }
        Some("--mic") => {
            let hint = a.get(4).filter(|s| !s.starts_with("--")).map(|s| s.as_str());
            let secs: u64 = a.iter().position(|x| x == "--seconds").and_then(|i| a.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(30);
            let (_stream, rx, name) = audio::capture(hint)?;
            eprintln!("capturing from {name} for {secs} s");
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(secs) {
                if let Ok(block) = rx.recv_timeout(Duration::from_millis(200)) {
                    for (c, t) in ch.push(&block)? {
                        print(&c, &t);
                    }
                }
            }
        }
        _ => anyhow::bail!("usage: ls-hear <model> <vad> --wav f.wav [--realtime] | --mic [device] [--seconds N] | --list"),
    }
    Ok(())
}
