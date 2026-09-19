//! Spike S3: whisper-rs + Silero VAD latency on this Mac.
//! Usage: spike-whisper <ggml-model> <vad-model> <wav 16k mono i16>
//! Measures: model load, VAD over the whole file, and transcription latency for
//! growing prefixes (1 s, 2 s, 4 s, 6 s, 8 s) — the `curr` re-transcription pattern.
use std::time::Instant;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (model, vad_model, wav) = (&args[1], &args[2], &args[3]);

    let samples: Vec<f32> = hound::WavReader::open(wav)?
        .into_samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    println!("audio: {:.1} s", samples.len() as f32 / 16000.0);

    let t = Instant::now();
    let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())?;
    let mut state = ctx.create_state()?;
    println!("load {}: {} ms", model, t.elapsed().as_millis());

    let t = Instant::now();
    let mut vparams = WhisperVadContextParams::new();
    vparams.set_n_threads(2);
    let mut vad = WhisperVadContext::new(vad_model, vparams)?;
    println!("vad load: {} ms", t.elapsed().as_millis());
    let t = Instant::now();
    let segs = vad.segments_from_samples(WhisperVadParams::new(), &samples)?;
    let n = segs.num_segments();
    let mut spans = vec![];
    for i in 0..n {
        spans.push((
            segs.get_segment_start_timestamp(i).unwrap_or(0.0),
            segs.get_segment_end_timestamp(i).unwrap_or(0.0),
        ));
    }
    println!("vad full file: {} ms, {} segments {:?}", t.elapsed().as_millis(), n, spans);

    // warm-up
    transcribe(&mut state, &samples[..16000])?;

    for secs in [1usize, 2, 4, 6, 8] {
        let end = (secs * 16000).min(samples.len());
        let mut best = u128::MAX;
        let mut text = String::new();
        for _ in 0..3 {
            let t = Instant::now();
            text = transcribe(&mut state, &samples[..end])?;
            best = best.min(t.elapsed().as_millis());
        }
        println!("prefix {secs} s: best {best} ms  | {}", text.trim());
    }
    Ok(())
}

fn transcribe(state: &mut whisper_rs::WhisperState, pcm: &[f32]) -> anyhow::Result<String> {
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    p.set_language(Some("en"));
    p.set_n_threads(4);
    p.set_no_context(true);
    p.set_single_segment(true);
    p.set_print_special(false);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
    state.full(p, pcm)?;
    let mut out = String::new();
    for seg in state.as_iter() {
        out.push_str(&seg.to_str_lossy()?);
    }
    Ok(out)
}
