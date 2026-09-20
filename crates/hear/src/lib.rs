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
        // min_silence 700: presenters pause mid-sentence for breath, and at 600 ms that split the
        // sentence in two (18% of finals on 09-19 were fragments under 1.5 s). whisper.cpp merges VAD
        // segments whose gap is < 200 ms and shrinks each reported gap by 2× speech_pad, so the effective
        // threshold is ~900 ms — inside the 700–900 ms band LiveKit and Deepgram use for end-of-turn.
        // Waiting longer costs nothing on screen: partials keep flowing while the pause runs.
        Self { tick_ms: 500, min_silence_ms: 700, max_chunk_ms: 8_000, min_speech_ms: 400 }
    }
}

/// Timing of one ASR pass, for the latency log.
#[derive(Debug, Clone)]
pub struct ChunkTiming {
    pub audio_ms: u64,
    pub vad_ms: f64,
    pub asr_ms: f64,
    /// RMS over speech frames. Diagnostic only: applying gain to reach a target level was tried on
    /// 09-19 and changed nothing, because Whisper normalises its own log-mel features. Logged so a
    /// quiet mic is visible as a number rather than inferred from a bad transcript. Takes that
    /// transcribed well sat at 0.145–0.169; the take that came back as garble sat at 0.050.
    pub speech_rms: f32,
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
        let end = end.min(self.buf.len());
        Ok(self.finalize(end, self.buf.len(), vad_ms)?.into_iter().collect())
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
            // Drop the gap too, keeping 200 ms of lead-in before the next utterance: silence left at the
            // head of the buffer becomes the leading audio of the next decode, and Whisper hallucinates
            // into it (`max_initial_ts` forces the first timestamp into the window's first second).
            let lead = 0.2_f32;
            let drain_to = (((w[1].0 - lead).max(0.0) * SR as f32) as usize).clamp(end_sample, self.buf.len());
            return Ok(self.finalize(end_sample, drain_to, vad_ms)?.into_iter().collect());
        }
        let speech_start = (spans[0].0 * 1000.0) as u64;
        let speech_end = (spans.last().unwrap().1 * 1000.0) as u64;
        let silence_after = buf_ms.saturating_sub(speech_end);

        if silence_after >= self.cfg.min_silence_ms {
            // Drop the silence that triggered this finalize, but never the newest 300 ms: Silero misses
            // quiet or overlapping speech at the live edge, and audio the VAD did not flag is still audio.
            // Draining the whole buffer here deleted words outright for a soft second speaker.
            let end_sample = ((speech_end as usize) * SR / 1000).min(self.buf.len());
            let drain_to = self.buf.len().saturating_sub(SR * 300 / 1000).max(end_sample);
            return Ok(self.finalize(end_sample, drain_to, vad_ms)?.into_iter().collect());
        }
        if buf_ms >= self.cfg.max_chunk_ms {
            // A cut at the wall clock bisects whatever word is in flight — 8% of finals on 09-19, one of
            // them ending "...telling me to like...". Cut at the last inter-word silence of >= 100 ms
            // instead, which is what Silero itself does at its `max_speech_duration_s` cap. Blind only if
            // the speaker genuinely never paused.
            let cut = spans
                .windows(2)
                .filter(|w| ((w[1].0 - w[0].1) * 1000.0) >= 100.0)
                .next_back()
                .map(|w| (w[0].1 * SR as f32) as usize)
                .unwrap_or(self.buf.len())
                .min(self.buf.len());
            return Ok(self.finalize(cut, cut, vad_ms)?.into_iter().collect());
        }
        if speech_end.saturating_sub(speech_start) < self.cfg.min_speech_ms {
            return Ok(vec![]);
        }
        let t = std::time::Instant::now();
        let level = speech_rms(&self.buf);
        let text = clean(&self.asr.transcribe(&self.buf)?);
        let asr_ms = t.elapsed().as_secs_f64() * 1000.0;
        if text.is_empty() || text == self.last_curr {
            return Ok(vec![]);
        }
        let stable = agreed_prefix(&self.last_curr, &text);
        self.last_curr = text.clone();
        let c = Chunk {
            id: self.take_id(),
            text,
            stable,
            t_start_ms: Self::ms(self.buf_start) + speech_start,
            t_end_ms: Self::ms(self.total),
            is_final: false,
        };
        Ok(vec![(c, ChunkTiming { audio_ms: Self::ms(self.total), vad_ms, asr_ms, speech_rms: level })])
    }

    /// `end_sample` is where the speech ends; `drain_to` is how much of the buffer this utterance
    /// consumes (>= `end_sample`, so trailing silence does not become the next decode's leading audio).
    fn finalize(&mut self, end_sample: usize, drain_to: usize, vad_ms: f64) -> anyhow::Result<Option<(Chunk, ChunkTiming)>> {
        let t = std::time::Instant::now();
        // Decode a guard band past the VAD's idea of the end. Silero's 100 ms pad is tuned for
        // segmentation, not for giving a transformer acoustic context, and a word's release (plosives,
        // final /s/) lands after the energy-based endpoint — cutting there clips the last word.
        let decode_to = (end_sample + SR * 250 / 1000).min(self.buf.len());
        let level = speech_rms(&self.buf[..decode_to]);
        let text = clean(&self.asr.transcribe(&self.buf[..decode_to])?);
        let asr_ms = t.elapsed().as_secs_f64() * 1000.0;
        let start_ms = Self::ms(self.buf_start);
        let end_ms = start_ms + Self::ms(end_sample as u64);
        let stable = agreed_prefix(&self.last_curr, &text);
        let drain_to = drain_to.clamp(end_sample, self.buf.len());
        self.buf.drain(..drain_to);
        self.buf_start += drain_to as u64;
        self.last_curr.clear();
        if text.is_empty() {
            return Ok(None);
        }
        let c = Chunk { id: self.take_id(), text, stable, t_start_ms: start_ms, t_end_ms: end_ms, is_final: true };
        Ok(Some((c, ChunkTiming { audio_ms: Self::ms(self.total), vad_ms, asr_ms, speech_rms: level })))
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// RMS of the loudest 30% of 20 ms frames — the speech, not the room between words.
fn speech_rms(pcm: &[f32]) -> f32 {
    let fr = SR / 50;
    if pcm.len() < fr * 4 {
        return 0.0;
    }
    let mut frames: Vec<f32> = pcm
        .chunks_exact(fr)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / fr as f32).sqrt())
        .collect();
    frames.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let hi = &frames[frames.len() * 7 / 10..];
    if hi.is_empty() { 0.0 } else { hi.iter().sum::<f32>() / hi.len() as f32 }
}

/// The leading words this decode agrees on with the decode before it.
///
/// Whisper re-transcribes the whole utterance every tick, so this is cross-decode agreement — the same
/// signal `LocalAgreement-2` uses (Macháček et al. 2023). A word the audio really contains survives the
/// re-decode; a hallucinated one does not. It is a far better confidence measure than Whisper's own
/// probabilities, which stay high precisely when it hallucinates. On 09-19 "I'll get it, okay, it's done."
/// was followed by "I'll get that guy's gun." — they agree on one word, and "gun" is not in it.
pub fn agreed_prefix(prev: &str, curr: &str) -> String {
    let norm = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase();
    let p: Vec<&str> = prev.split_whitespace().collect();
    let c: Vec<&str> = curr.split_whitespace().collect();
    let n = p.iter().zip(c.iter()).take_while(|(a, b)| norm(a) == norm(b)).count();
    c[..n].join(" ")
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
    let out = collapse_repeats(&out.split_whitespace().collect::<Vec<_>>().join(" "));
    let lower = out.to_lowercase();
    if ["", ".", "you", "you.", "thanks for watching!"].contains(&lower.as_str()) {
        return String::new();
    }
    out
}

/// Whisper on noisy/unclear audio loops: "it's just, it's just, it's just…", "oh, oh, oh…", "blue rose,
/// blue rose…" (live test 09-19). Any 1–4 word run repeated 3+ times in a row is kept once.
pub fn collapse_repeats(s: &str) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let norm: Vec<String> = words
        .iter()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .collect();
    let mut keep: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    'outer: while i < words.len() {
        for n in 1..=4 {
            if i + 3 * n > words.len() || norm[i..i + n].iter().all(|w| w.is_empty()) {
                continue;
            }
            let mut k = 1;
            while i + (k + 1) * n <= words.len() && norm[i + k * n..i + (k + 1) * n] == norm[i..i + n] {
                k += 1;
            }
            if k >= 3 {
                keep.extend_from_slice(&words[i..i + n]);
                i += k * n;
                continue 'outer;
            }
        }
        keep.push(words[i]);
        i += 1;
    }
    keep.join(" ")
}

/// Default Whisper audio-context floor in frames (50/s); see WhisperAsr::transcribe.
pub const AUDIO_CTX_FLOOR: i32 = 768;

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
        prompt: Option<String>,
    }

    impl WhisperAsr {
        pub fn load(model: &str) -> anyhow::Result<Self> {
            whisper_rs::install_logging_hooks(); // silence whisper.cpp/ggml stderr spam
            let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())?;
            let state = ctx.create_state()?;
            let mut s = Self { _ctx: ctx, state, prompt: None };
            let _ = s.transcribe(&vec![0.0; super::SR]); // warm-up (Metal init)
            Ok(s)
        }

        /// Bias recognition toward the image library's words (Whisper initial prompt). Kept short.
        pub fn with_vocabulary(mut self, words: &[String]) -> Self {
            let mut uniq: Vec<&String> = vec![];
            for w in words {
                if !w.is_empty() && !uniq.contains(&w) {
                    uniq.push(w);
                }
            }
            if !uniq.is_empty() {
                let list: Vec<&str> = uniq.iter().take(80).map(|s| s.as_str()).collect();
                self.prompt = Some(format!("Talk mentioning: {}.", list.join(", ")));
            }
            self
        }
    }

    /// One pass with the confidence whisper.cpp already computes: per-token probability and the
    /// segment's no-speech probability. Used to refuse acting on words that were barely heard.
    pub struct Heard {
        pub text: String,
        /// (token text, probability) in order.
        pub tokens: Vec<(String, f32)>,
        pub no_speech: f32,
    }

    impl WhisperAsr {
        pub fn transcribe_detailed(&mut self, pcm: &[f32]) -> anyhow::Result<Heard> {
            let text = <Self as Asr>::transcribe(self, pcm)?;
            let (mut tokens, mut no_speech) = (vec![], 0.0f32);
            for seg in self.state.as_iter() {
                no_speech = no_speech.max(seg.no_speech_probability());
                for i in 0..seg.n_tokens() {
                    if let Some(tok) = seg.get_token(i) {
                        if let Ok(t) = tok.to_str_lossy() {
                            tokens.push((t.to_string(), tok.token_probability()));
                        }
                    }
                }
            }
            Ok(Heard { text, tokens, no_speech })
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
            p.set_suppress_nst(true); // no "(laughs)", "*music*" style non-speech tokens
            // Speech is ~3–4 tokens/s; a hard cap per window stops runaway repetition loops.
            p.set_max_tokens((pcm.len() as f32 / super::SR as f32 * 6.0).ceil() as i32 + 6);
            if let Some(prompt) = &self.prompt {
                p.set_initial_prompt(prompt);
            }
            // No temperature fallback: on short/low-confidence audio it re-decodes up to 5× (measured
            // 2.2 s spikes). One greedy pass is enough; the next tick re-transcribes anyway.
            p.set_temperature(0.0);
            p.set_temperature_inc(0.0);
            // Whisper needs ≥ 1 s; pad short buffers with silence.
            let padded;
            let pcm = if pcm.len() < super::SR + 1600 {
                padded = [pcm, &vec![0.0; super::SR + 1600 - pcm.len()]].concat();
                &padded[..]
            } else {
                pcm
            };
            // Encode less than the padded 30 s window (the encoder dominates cost on 1–8 s windows):
            // audio_ctx = max(floor, real frames + margin). Too small a context hurts accuracy in
            // noise, so there is a floor. WHISPER_AUDIO_CTX=<floor frames, 50/s>, 0 = full 1500.
            let floor: i32 = std::env::var("WHISPER_AUDIO_CTX").ok().and_then(|v| v.parse().ok()).unwrap_or(super::AUDIO_CTX_FLOOR);
            if floor > 0 {
                let frames = (pcm.len() as f32 / super::SR as f32 * 50.0).ceil() as i32;
                p.set_audio_ctx((((frames + 64).max(floor) + 63) / 64 * 64).min(1500));
            }
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
            p.set_use_gpu(false); // tiny model; keep Metal free for Whisper
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
        // Device lookup can block inside CoreAudio (seen with the iPhone Continuity mic and when mic
        // permission is pending), so resolve it on a helper thread with a timeout.
        let hint = device_hint.map(|h| h.to_lowercase());
        // At most one lookup in flight: if an earlier one is still blocked (e.g. on the permission
        // prompt), wait on it again instead of piling up blocked threads on every retry.
        static PENDING: std::sync::Mutex<Option<(Option<String>, std::sync::mpsc::Receiver<Option<cpal::Device>>)>> =
            std::sync::Mutex::new(None);
        let mut pending = PENDING.lock().unwrap();
        let reuse = matches!(&*pending, Some((h, _)) if *h == hint);
        if !reuse {
            let (dtx, drx) = std::sync::mpsc::channel();
            let hint2 = hint.clone();
            std::thread::spawn(move || {
                let host = cpal::default_host();
                let dev = match &hint2 {
                    Some(h) => host
                        .input_devices()
                        .ok()
                        .and_then(|mut it| it.find(|d| d.name().map(|n| n.to_lowercase().contains(h)).unwrap_or(false))),
                    // No hint: prefer AirPods, then the built-in mic; never a virtual loopback (BlackHole,
                    // Teams Audio…) that happens to be the system default and would hear silence.
                    None => {
                        let devs: Vec<_> = host.input_devices().map(|it| it.collect()).unwrap_or_default();
                        let name = |d: &cpal::Device| d.name().unwrap_or_default().to_lowercase();
                        let virtual_dev = |n: &str| ["blackhole", "teams", "zoom", "loopback", "soundflower", "aggregate"].iter().any(|v| n.contains(v));
                        let pos = |want: &str| devs.iter().position(|d| name(d).contains(want));
                        pos("airpods")
                            .or_else(|| pos("macbook"))
                            .or_else(|| devs.iter().position(|d| !virtual_dev(&name(d))))
                            .map(|i| devs[i].clone())
                            .or_else(|| host.default_input_device())
                    }
                };
                let _ = dtx.send(dev);
            });
            *pending = Some((hint.clone(), drx));
        }
        let (h, drx) = pending.take().expect("lookup just ensured");
        let dev = match drx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(d) => d.ok_or_else(|| anyhow::anyhow!("no input device matching {device_hint:?} (see `ls-hear --list`)"))?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                *pending = Some((h, drx)); // still blocked: the next call waits on this same lookup
                anyhow::bail!("audio device lookup timed out after 5 s (mic permission pending? device asleep?)")
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => anyhow::bail!("audio device lookup failed"),
        };
        drop(pending);
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
        assert_eq!(clean("Thank you."), "Thank you.", "a real closing can render as a text card");
        assert_eq!(clean(" Hello (music) world "), "Hello world");
        assert_eq!(clean("Oh, it's just, it's just, it's just, it's just so exciting."), "Oh, it's just, so exciting.");
        assert_eq!(clean("what, what, what, what, what"), "what,");
        assert_eq!(clean("the eagle, white rose, blue rose, blue rose, blue rose, blue rose"), "the eagle, white rose, blue rose");
        assert_eq!(clean("very very good"), "very very good"); // two in a row is real speech
    }

    #[test]
    fn resample_halves_length() {
        let x: Vec<f32> = (0..32000).map(|i| i as f32).collect();
        assert_eq!(audio::resample(&x, 32000, 16000).len(), 16000);
        assert_eq!(audio::downmix(&[1.0, 3.0, 2.0, 4.0], 2), vec![2.0, 3.0]);
    }
}
