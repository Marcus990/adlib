//! Track A: audio → VAD → Whisper → `Chunk` events (design doc §4 "ASR + chunking").
//!
//! The `Chunker` consumes 16 kHz mono f32 audio. Every `tick_ms` of new audio it runs VAD over the
//! current utterance buffer and either:
//!  - re-transcribes the in-progress speech and emits `curr` (`is_final = false`) if the text changed,
//!  - or, after `min_silence_ms` of silence following speech (or `max_chunk_ms`), emits the final
//!    chunk (`is_final = true`) and starts a new utterance.
//! Every emitted event gets a fresh id, because each one fans out to its own Jev + query calls and
//! the stage joins by id. Time is the audio clock (ms since stream start), so WAV replay and live
//! capture produce the same event format.

use ls_contracts::Chunk;

pub const SR: usize = 16_000;

pub trait Asr {
    fn transcribe(&mut self, pcm: &[f32]) -> anyhow::Result<String>;
}

pub trait Vad {
    /// Speech spans in seconds, relative to the start of `pcm`.
    fn speech_spans(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<(f32, f32)>>;
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkerConfig {
    pub tick_ms: u64,
    pub min_silence_ms: u64,
    pub max_chunk_ms: u64,
    /// Minimum speech before we bother transcribing `curr`.
    pub min_speech_ms: u64,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self { tick_ms: 750, min_silence_ms: 600, max_chunk_ms: 8_000, min_speech_ms: 400 }
    }
}

/// Timing of one ASR pass, for the latency log.
#[derive(Debug, Clone)]
pub struct ChunkTiming {
    pub audio_ms: u64,
    pub vad_ms: f64,
    pub asr_ms: f64,
}

pub struct Chunker<A: Asr, V: Vad> {
    pub cfg: ChunkerConfig,
    asr: A,
    vad: V,
    buf: Vec<f32>,
    /// Audio-clock position (samples) of buf[0].
    buf_start: u64,
    /// Total samples received.
    total: u64,
    since_tick: u64,
    next_id: u64,
    last_curr: String,
}

impl<A: Asr, V: Vad> Chunker<A, V> {
    pub fn new(cfg: ChunkerConfig, asr: A, vad: V) -> Self {
        Self { cfg, asr, vad, buf: vec![], buf_start: 0, total: 0, since_tick: 0, next_id: 1, last_curr: String::new() }
    }

    fn ms(samples: u64) -> u64 {
        samples * 1000 / SR as u64
    }

    /// Feed audio; returns any chunk events produced (0–2), each with its timing.
    pub fn push(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<(Chunk, ChunkTiming)>> {
        self.buf.extend_from_slice(pcm);
        self.total += pcm.len() as u64;
        self.since_tick += pcm.len() as u64;
        if Self::ms(self.since_tick) < self.cfg.tick_ms {
            return Ok(vec![]);
        }
        self.since_tick = 0;
        self.tick()
    }

    /// Flush at end of stream: finalise whatever speech remains.
    pub fn finish(&mut self) -> anyhow::Result<Vec<(Chunk, ChunkTiming)>> {
        let t = std::time::Instant::now();
        let spans = self.vad.speech_spans(&self.buf)?;
        let vad_ms = t.elapsed().as_secs_f64() * 1000.0;
        if spans.is_empty() {
            return Ok(vec![]);
        }
        let end = (spans.last().unwrap().1 * SR as f32) as usize;
        Ok(self.finalize(end.min(self.buf.len()), vad_ms)?.into_iter().collect())
    }

    fn tick(&mut self) -> anyhow::Result<Vec<(Chunk, ChunkTiming)>> {
        let t = std::time::Instant::now();
        let spans = self.vad.speech_spans(&self.buf)?;
        let vad_ms = t.elapsed().as_secs_f64() * 1000.0;
        let buf_ms = Self::ms(self.buf.len() as u64);

        if spans.is_empty() {
            // Only silence: keep the last 500 ms so a word onset isn't cut.
            let keep = SR / 2;
            if self.buf.len() > keep * 2 {
                let drop = self.buf.len() - keep;
                self.buf.drain(..drop);
                self.buf_start += drop as u64;
            }
            return Ok(vec![]);
        }
        // A pause that ended between ticks shows up as an internal gap between spans: finalize the
        // earlier utterance at the gap so two utterances never merge.
        if let Some(w) = spans.windows(2).find(|w| ((w[1].0 - w[0].1) * 1000.0) as u64 >= self.cfg.min_silence_ms) {
            let end_sample = ((w[0].1 * SR as f32) as usize).min(self.buf.len());
            return Ok(self.finalize(end_sample, vad_ms)?.into_iter().collect());
        }
        let speech_start = (spans[0].0 * 1000.0) as u64;
        let speech_end = (spans.last().unwrap().1 * 1000.0) as u64;
        let silence_after = buf_ms.saturating_sub(speech_end);

        if silence_after >= self.cfg.min_silence_ms || buf_ms >= self.cfg.max_chunk_ms {
            let end = if silence_after >= self.cfg.min_silence_ms { speech_end } else { buf_ms };
            let end_sample = ((end as usize) * SR / 1000).min(self.buf.len());
            return Ok(self.finalize(end_sample, vad_ms)?.into_iter().collect());
        }
        if speech_end.saturating_sub(speech_start) < self.cfg.min_speech_ms {
            return Ok(vec![]);
        }
        let t = std::time::Instant::now();
        let text = clean(&self.asr.transcribe(&self.buf)?);
        let asr_ms = t.elapsed().as_secs_f64() * 1000.0;
        if text.is_empty() || text == self.last_curr {
            return Ok(vec![]);
        }
        self.last_curr = text.clone();
        let c = Chunk {
            id: self.take_id(),
            text,
            t_start_ms: Self::ms(self.buf_start) + speech_start,
            t_end_ms: Self::ms(self.total),
            is_final: false,
        };
        Ok(vec![(c, ChunkTiming { audio_ms: Self::ms(self.total), vad_ms, asr_ms })])
    }

    fn finalize(&mut self, end_sample: usize, vad_ms: f64) -> anyhow::Result<Option<(Chunk, ChunkTiming)>> {
        let t = std::time::Instant::now();
        let text = clean(&self.asr.transcribe(&self.buf[..end_sample])?);
        let asr_ms = t.elapsed().as_secs_f64() * 1000.0;
        let start_ms = Self::ms(self.buf_start);
        let end_ms = start_ms + Self::ms(end_sample as u64);
        self.buf.drain(..end_sample);
        self.buf_start += end_sample as u64;
        self.last_curr.clear();
        if text.is_empty() {
            return Ok(None);
        }
        let c = Chunk { id: self.take_id(), text, t_start_ms: start_ms, t_end_ms: end_ms, is_final: true };
        Ok(Some((c, ChunkTiming { audio_ms: Self::ms(self.total), vad_ms, asr_ms })))
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Strip Whisper's non-speech markers and whitespace.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = out.to_lowercase();
    if ["", ".", "you", "you.", "thank you.", "thanks for watching!"].contains(&lower.as_str()) {
        return String::new();
    }
    out
}

pub mod whisper {
    //! Real ASR + VAD backed by whisper.cpp (whisper-rs). base.en chosen by spike S3.
    use super::{Asr, Vad};
    use whisper_rs::{
        FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState, WhisperVadContext,
        WhisperVadContextParams, WhisperVadParams,
    };

    pub struct WhisperAsr {
        _ctx: WhisperContext,
        state: WhisperState,
    }

    impl WhisperAsr {
        pub fn load(model: &str) -> anyhow::Result<Self> {
            let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())?;
            let state = ctx.create_state()?;
            let mut s = Self { _ctx: ctx, state };
            let _ = s.transcribe(&vec![0.0; super::SR]); // warm-up (Metal init)
            Ok(s)
        }
    }

    impl Asr for WhisperAsr {
        fn transcribe(&mut self, pcm: &[f32]) -> anyhow::Result<String> {
            let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            p.set_language(Some("en"));
            p.set_n_threads(4);
            p.set_no_context(true);
            p.set_single_segment(true);
            p.set_print_special(false);
            p.set_print_progress(false);
            p.set_print_realtime(false);
            p.set_print_timestamps(false);
            p.set_suppress_blank(true);
            // Whisper needs ≥ 1 s; pad short buffers with silence.
            let padded;
            let pcm = if pcm.len() < super::SR + 1600 {
                padded = [pcm, &vec![0.0; super::SR + 1600 - pcm.len()]].concat();
                &padded[..]
            } else {
                pcm
            };
            self.state.full(p, pcm)?;
            let mut out = String::new();
            for seg in self.state.as_iter() {
                out.push_str(&seg.to_str_lossy()?);
            }
            Ok(out)
        }
    }

    pub struct SileroVad {
        ctx: WhisperVadContext,
    }

    impl SileroVad {
        pub fn load(model: &str) -> anyhow::Result<Self> {
            let mut p = WhisperVadContextParams::new();
            p.set_n_threads(2);
            Ok(Self { ctx: WhisperVadContext::new(model, p)? })
        }
    }

    impl Vad for SileroVad {
        fn speech_spans(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<(f32, f32)>> {
            if pcm.len() < 512 {
                return Ok(vec![]);
            }
            let mut params = WhisperVadParams::new();
            params.set_min_silence_duration(300);
            params.set_speech_pad(100);
            let segs = self.ctx.segments_from_samples(params, pcm)?;
            let mut out = vec![];
            for i in 0..segs.num_segments() {
                // whisper.cpp VAD timestamps are centiseconds.
                let s = segs.get_segment_start_timestamp(i).unwrap_or(0.0) / 100.0;
                let e = segs.get_segment_end_timestamp(i).unwrap_or(0.0) / 100.0;
                out.push((s, e));
            }
            Ok(out)
        }
    }
}

pub mod audio {
    //! WAV loading and live mic capture → 16 kHz mono f32.
    use super::SR;

    pub fn load_wav(path: &std::path::Path) -> anyhow::Result<Vec<f32>> {
        let mut r = hound::WavReader::open(path)?;
        let spec = r.spec();
        let raw: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => {
                let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
                r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()?
            }
            hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        };
        let mono = downmix(&raw, spec.channels as usize);
        Ok(resample(&mono, spec.sample_rate as usize, SR))
    }

    pub fn downmix(x: &[f32], ch: usize) -> Vec<f32> {
        if ch <= 1 {
            return x.to_vec();
        }
        x.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
    }

    /// Linear-interpolation resampler (adequate for speech → Whisper).
    pub fn resample(x: &[f32], from: usize, to: usize) -> Vec<f32> {
        if from == to || x.is_empty() {
            return x.to_vec();
        }
        let n = (x.len() as u64 * to as u64 / from as u64) as usize;
        (0..n)
            .map(|i| {
                let pos = i as f64 * from as f64 / to as f64;
                let j = pos.floor() as usize;
                let f = (pos - j as f64) as f32;
                let a = x[j.min(x.len() - 1)];
                let b = x[(j + 1).min(x.len() - 1)];
                a + (b - a) * f
            })
            .collect()
    }

    /// Start capturing from the named input device (substring match, e.g. "AirPods"), or the
    /// default input. Sends 16 kHz mono blocks on the returned channel. Keep the stream alive.
    pub fn capture(
        device_hint: Option<&str>,
    ) -> anyhow::Result<(cpal::Stream, std::sync::mpsc::Receiver<Vec<f32>>, String)> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let dev = match device_hint {
            Some(h) => host
                .input_devices()?
                .find(|d| d.name().map(|n| n.to_lowercase().contains(&h.to_lowercase())).unwrap_or(false))
                .ok_or_else(|| anyhow::anyhow!("no input device matching {h:?}"))?,
            None => host.default_input_device().ok_or_else(|| anyhow::anyhow!("no default input device"))?,
        };
        let name = dev.name().unwrap_or_default();
        let cfg = dev.default_input_config()?;
        let (ch, rate) = (cfg.channels() as usize, cfg.sample_rate().0 as usize);
        let (tx, rx) = std::sync::mpsc::channel();
        let err = |e| eprintln!("audio stream error: {e}");
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev.build_input_stream(
                &cfg.into(),
                move |d: &[f32], _: &_| {
                    let _ = tx.send(resample(&downmix(d, ch), rate, SR));
                },
                err,
                None,
            )?,
            cpal::SampleFormat::I16 => dev.build_input_stream(
                &cfg.into(),
                move |d: &[i16], _: &_| {
                    let f: Vec<f32> = d.iter().map(|v| *v as f32 / 32768.0).collect();
                    let _ = tx.send(resample(&downmix(&f, ch), rate, SR));
                },
                err,
                None,
            )?,
            other => anyhow::bail!("unsupported sample format {other:?}"),
        };
        stream.play()?;
        Ok((stream, rx, name))
    }

    pub fn list_inputs() -> Vec<String> {
        use cpal::traits::{DeviceTrait, HostTrait};
        cpal::default_host()
            .input_devices()
            .map(|it| it.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake ASR: reports how many seconds of non-zero audio it was given.
    struct FakeAsr;
    impl Asr for FakeAsr {
        fn transcribe(&mut self, pcm: &[f32]) -> anyhow::Result<String> {
            let secs = pcm.iter().filter(|x| **x != 0.0).count() / SR;
            Ok(format!("speech {secs}s"))
        }
    }
    /// Fake VAD: non-zero samples are speech.
    struct EnergyVad;
    impl Vad for EnergyVad {
        fn speech_spans(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<(f32, f32)>> {
            let mut spans = vec![];
            let mut start: Option<usize> = None;
            for (i, x) in pcm.iter().enumerate() {
                match (start, *x != 0.0) {
                    (None, true) => start = Some(i),
                    (Some(s), false) => {
                        spans.push((s as f32 / SR as f32, i as f32 / SR as f32));
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = start {
                spans.push((s as f32 / SR as f32, pcm.len() as f32 / SR as f32));
            }
            Ok(spans)
        }
    }

    fn feed(ch: &mut Chunker<FakeAsr, EnergyVad>, audio: &[f32]) -> Vec<Chunk> {
        let mut out = vec![];
        for block in audio.chunks(SR / 10) {
            out.extend(ch.push(block).unwrap().into_iter().map(|(c, _)| c));
        }
        out.extend(ch.finish().unwrap().into_iter().map(|(c, _)| c));
        out
    }

    #[test]
    fn curr_updates_then_final_on_silence() {
        let mut audio = vec![0.0f32; SR / 2];
        audio.extend(vec![0.5f32; SR * 3]); // 3 s speech
        audio.extend(vec![0.0f32; SR]); // 1 s silence
        let mut ch = Chunker::new(ChunkerConfig::default(), FakeAsr, EnergyVad);
        let ev = feed(&mut ch, &audio);
        let currs: Vec<_> = ev.iter().filter(|c| !c.is_final).collect();
        let finals: Vec<_> = ev.iter().filter(|c| c.is_final).collect();
        assert!(currs.len() >= 2, "expected several curr updates, got {ev:?}");
        assert_eq!(finals.len(), 1, "{ev:?}");
        assert_eq!(finals[0].text, "speech 3s");
        assert!(finals[0].t_end_ms >= 3400 && finals[0].t_end_ms <= 3600, "{:?}", finals[0]);
        // ids are unique and increasing
        let ids: Vec<u64> = ev.iter().map(|c| c.id).collect();
        assert!(ids.windows(2).all(|w| w[1] > w[0]), "{ids:?}");
    }

    #[test]
    fn two_utterances_give_two_finals() {
        let mut audio = vec![0.5f32; SR * 2];
        audio.extend(vec![0.0f32; SR]);
        audio.extend(vec![0.5f32; SR * 2]);
        audio.extend(vec![0.0f32; SR]);
        let mut ch = Chunker::new(ChunkerConfig::default(), FakeAsr, EnergyVad);
        let finals: Vec<_> = feed(&mut ch, &audio).into_iter().filter(|c| c.is_final).collect();
        assert_eq!(finals.len(), 2, "{finals:?}");
        assert!(finals[1].t_start_ms >= 2000);
    }

    #[test]
    fn long_speech_is_force_finalized() {
        let audio = vec![0.5f32; SR * 12];
        let mut ch = Chunker::new(ChunkerConfig::default(), FakeAsr, EnergyVad);
        let finals: Vec<_> = feed(&mut ch, &audio).into_iter().filter(|c| c.is_final).collect();
        assert!(finals.len() >= 2, "{finals:?}");
    }

    #[test]
    fn silence_only_emits_nothing() {
        let mut ch = Chunker::new(ChunkerConfig::default(), FakeAsr, EnergyVad);
        assert!(feed(&mut ch, &vec![0.0f32; SR * 5]).is_empty());
    }

    #[test]
    fn clean_strips_markers_and_hallucinations() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean("Thank you."), "");
        assert_eq!(clean(" Hello (music) world "), "Hello world");
    }

    #[test]
    fn resample_halves_length() {
        let x: Vec<f32> = (0..32000).map(|i| i as f32).collect();
        assert_eq!(audio::resample(&x, 32000, 16000).len(), 16000);
        assert_eq!(audio::downmix(&[1.0, 3.0, 2.0, 4.0], 2), vec![2.0, 3.0]);
    }
}
