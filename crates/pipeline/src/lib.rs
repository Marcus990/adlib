//! Orchestration: Hear → (Decide ‖ Query→Search) → Stage → RenderSink, with a JSONL timing log
//! for every stage of every chunk (design doc §4, §6). Used by the Tauri app and by `ls-replay`.

use ls_agent::CanvasAgent;
use ls_canvas::{Canvas, Scene};
use ls_contracts::{Chunk, RenderEvent};
use ls_decide::{Decider, Source};
use ls_hear::{audio, whisper, ChunkTiming, Chunker, ChunkerConfig, SR};
use ls_query::QueryClient;
use ls_search::{Clip, ImageCache, Index, Searcher};
use ls_stage::{Outcome, SearchOutcome, Stage, StageConfig};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct Config {
    pub whisper_model: PathBuf,
    pub vad_model: PathBuf,
    pub clip_dir: PathBuf,
    pub index_path: PathBuf,
    pub api_key: Option<String>,
    pub query_model: Option<String>,
    pub jev_model: Option<String>,
    pub log_path: PathBuf,
    pub stage: StageConfig,
    pub chunker: ChunkerConfig,
    /// Canvas mode (evolving board + agent). LS_MODE=single keeps one full-screen image.
    pub canvas: bool,
    pub canvas_model: Option<String>,
}

impl Config {
    /// Defaults relative to the repo root; `.env` / env vars override the API settings.
    pub fn from_env(root: &std::path::Path) -> Self {
        let _ = dotenvy::from_path(root.join(".env"));
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let p = |k: &str, d: &str| env(k).map(PathBuf::from).unwrap_or_else(|| root.join(d));
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let mut stage = StageConfig::default();
        if let Some(t) = env("TAU").and_then(|v| v.parse().ok()) {
            stage.tau = t;
        }
        if env("CONFIRM").as_deref() == Some("0") {
            stage.confirm_partials = false;
        }
        Self {
            // small.en: 16% WER / 10 of 10 keywords on the noisy fixture vs base.en 46% / 4 of 10, at
            // 423 ms p50 with the audio-context floor (PROGRESS 09-19). Falls back to base.en if absent.
            whisper_model: p("WHISPER_MODEL", if root.join("models/ggml-small.en.bin").exists() { "models/ggml-small.en.bin" } else { "models/ggml-base.en.bin" }),
            vad_model: p("VAD_MODEL", "models/ggml-silero-v5.1.2.bin"),
            clip_dir: p("CLIP_DIR", "models/mobileclip-s2"),
            index_path: p("INDEX", "dev-library/index.json"),
            api_key: env("OPENROUTER_API_KEY"),
            query_model: env("QUERY_MODEL"),
            jev_model: env("JEV_MODEL"),
            log_path: root.join("logs").join(format!("run-{stamp}.jsonl")),
            stage,
            chunker: {
                let mut c = ChunkerConfig::default();
                c.tick_ms = 600; // small.en p90 ≈ 640 ms per pass; a 500 ms tick would fall behind
                if let Some(t) = env("ASR_TICK_MS").and_then(|v| v.parse().ok()) {
                    c.tick_ms = t;
                }
                c
            },
            canvas: env("LS_MODE").map(|m| m != "single").unwrap_or(true),
            canvas_model: env("CANVAS_MODEL"),
        }
    }
}

pub enum AudioSource {
    Wav { path: PathBuf, realtime: bool },
    Mic { device: Option<String> },
}

pub trait RenderSink: Send + Sync + 'static {
    fn render(&self, ev: &RenderEvent);
    fn status(&self, _v: &Value) {}
    /// Canvas mode: the whole board after every change.
    fn scene(&self, _s: &Scene) {}
}

/// Append-only JSONL log; every line gets `t_ms` (ms since pipeline start).
#[derive(Clone)]
pub struct Logger {
    t0: Instant,
    out: Arc<Mutex<std::io::BufWriter<std::fs::File>>>,
    pub path: std::path::PathBuf,
}

impl Logger {
    pub fn create(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        Ok(Self { t0: Instant::now(), out: Arc::new(Mutex::new(std::io::BufWriter::new(std::fs::File::create(path)?))), path: path.to_path_buf() })
    }
    pub fn now_ms(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }
    pub fn log(&self, mut v: Value) {
        v["t_ms"] = json!(self.now_ms());
        let mut o = self.out.lock().unwrap();
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    }
}

/// Loaded models and clients; build once, reuse across runs.
pub struct Engine {
    pub cfg: Config,
    pub clip: Arc<Clip>,
    pub searcher: Arc<Searcher>,
    pub cache: ImageCache,
    pub decider: Decider,
    pub query: QueryClient,
    pub agent: CanvasAgent,
}

impl Engine {
    pub async fn load(cfg: Config) -> anyhow::Result<Self> {
        let clip = Arc::new(Clip::load(&cfg.clip_dir)?);
        let index = Index::load(&cfg.index_path)?;
        let captions = index.captions();
        let cache = ImageCache::new(&index, 256 * 1024 * 1024);
        let searcher = Arc::new(Searcher::new(index));
        let http = reqwest::Client::builder().pool_idle_timeout(Duration::from_secs(300)).tcp_keepalive(Duration::from_secs(30)).build()?;
        let query_model = cfg.query_model.clone().unwrap_or_else(|| ls_query::DEFAULT_MODEL.to_string());
        let decider = Decider::new(http.clone(), cfg.api_key.clone(), cfg.jev_model.clone(), query_model.clone(), captions.clone());
        let query = QueryClient::new(http.clone(), cfg.api_key.clone(), Some(query_model), captions);
        let agent = CanvasAgent::new(http, cfg.api_key.clone(), cfg.canvas_model.clone());
        Ok(Self { cfg, clip, searcher, cache, decider, query, agent })
    }

    /// Warm-up (design doc §6 lever 5): CLIP text path, image prefetch, both HTTPS connections.
    pub async fn warm_up(&self, log: &Logger) {
        let t = Instant::now();
        let _ = self.clip.embed_text("warm up");
        let n = self.cache.prefetch_all();
        tokio::join!(self.decider.warm_up(), self.query.warm_up());
        log.log(json!({"ev": "warm_up", "ms": t.elapsed().as_millis() as u64, "prefetched": n,
            "remote": self.decider.has_remote()}));
    }
}

enum Msg {
    Chunk(Chunk, ChunkTiming, u64 /* wall ms when emitted */),
    AudioStart(u64),
    HearDone,
    HearError(String),
    Status(Value),
    Decision(ls_contracts::ChangeDecision, Source, u64, u64),
    Search(SearchOutcome, ls_contracts::QueryResult, Vec<ls_search::PhraseHit>, u64, u64, u64),
    /// Canvas agent answer: (scene version it reasoned about, ops, source, chunk id, ms).
    Agent(u64, Vec<ls_canvas::Op>, ls_agent::Source, u64, u64),
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Summary {
    pub chunks: usize,
    pub renders: usize,
    pub render_latencies_ms: Vec<u64>,
    pub outcomes: std::collections::BTreeMap<String, usize>,
    pub decide_sources: std::collections::BTreeMap<String, usize>,
    pub query_fallbacks: usize,
    pub shown: Vec<String>,
}

impl Summary {
    pub fn pct(&self, p: f64) -> Option<u64> {
        let mut v = self.render_latencies_ms.clone();
        if v.is_empty() {
            return None;
        }
        v.sort();
        Some(v[((v.len() - 1) as f64 * p).round() as usize])
    }
}

fn outcome_name(o: &Outcome) -> &'static str {
    match o {
        Outcome::Rendered(_) => "rendered",
        Outcome::Pending => "pending",
        Outcome::NoChange => "no_change",
        Outcome::BelowProbability => "below_probability",
        Outcome::LibraryGap => "library_gap",
        Outcome::Duplicate => "duplicate",
        Outcome::Stale => "stale",
        Outcome::TimedOut => "timed_out",
        Outcome::Unconfirmed => "unconfirmed",
    }
}

/// Compact scene line for the run log (eval + debugging): tile kinds and graphic contents.
fn scene_log(scene: &Scene) -> Value {
    let tiles: Vec<Value> = scene
        .elements
        .iter()
        .map(|e| match (&e.diagram, &e.chart) {
            (Some(d), _) => json!({"id": e.id, "kind": "diagram", "layout": d.layout, "title": d.title,
                "nodes": d.nodes.iter().map(|n| n.label.clone()).collect::<Vec<_>>(), "edges": d.edges.len()}),
            (_, Some(c)) => json!({"id": e.id, "kind": "chart", "chart": c.kind, "title": c.title, "unit": c.unit,
                "points": c.points.iter().map(|p| json!([p.label, p.value])).collect::<Vec<_>>()}),
            _ => json!({"id": e.id, "kind": "image", "image_id": e.image_id}),
        })
        .collect();
    json!({"ev": "scene", "version": scene.version, "reason": scene.reason, "n": scene.elements.len(), "layout": scene.layout, "tiles": tiles})
}

/// Run one talk until the audio ends (WAV) or `stop` is set (mic).
pub async fn run(engine: Arc<Engine>, source: AudioSource, sink: Arc<dyn RenderSink>, log: Logger, stop: Arc<AtomicBool>) -> anyhow::Result<Summary> {
    let cfg = engine.cfg.clone();
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

    // Library subjects as a Whisper vocabulary hint — opt-in (WHISPER_VOCAB=1): on noisy audio Whisper
    // emits prompt words ("white rose, blue rose, blue rose…", live test 09-19).
    let vocab: Vec<String> = if std::env::var("WHISPER_VOCAB").as_deref() == Ok("1") {
        engine.searcher.index.entries.iter().map(|e| e.caption.split('(').next().unwrap_or(&e.caption).trim().to_string()).collect()
    } else {
        vec![]
    };
    // ---- Track A on its own OS thread (Whisper is blocking). ----
    {
        let tx = tx.clone();
        let log = log.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = (|| -> anyhow::Result<()> {
                let t = Instant::now();
                let asr = whisper::WhisperAsr::load(cfg.whisper_model.to_str().unwrap())?.with_vocabulary(&vocab);
                let vad = whisper::SileroVad::load(cfg.vad_model.to_str().unwrap())?;
                log.log(json!({"ev": "hear_loaded", "ms": t.elapsed().as_millis() as u64}));
                let mut ch = Chunker::new(cfg.chunker, asr, vad);
                let emit = |evs: Vec<(Chunk, ChunkTiming)>| {
                    for (c, tm) in evs {
                        let _ = tx.send(Msg::Chunk(c, tm, log.now_ms()));
                    }
                };
                match source {
                    AudioSource::Wav { path, realtime } => {
                        let pcm = audio::load_wav(&path)?;
                        let start = Instant::now();
                        let _ = tx.send(Msg::AudioStart(log.now_ms()));
                        for (i, block) in pcm.chunks(SR / 10).enumerate() {
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                            if realtime {
                                if let Some(w) = Duration::from_millis(i as u64 * 100).checked_sub(start.elapsed()) {
                                    std::thread::sleep(w);
                                }
                            }
                            emit(ch.push(block)?);
                        }
                        emit(ch.finish()?);
                    }
                    AudioSource::Mic { device } => {
                        // Keep retrying: the first launch of the app waits on macOS's microphone
                        // permission prompt, and a Bluetooth mic can take a moment to appear.
                        let (_stream, arx, name) = loop {
                            match audio::capture(device.as_deref()) {
                                Ok(c) => break c,
                                Err(e) => {
                                    if stop.load(Ordering::Relaxed) {
                                        return Ok(());
                                    }
                                    log.log(json!({"ev": "mic_retry", "error": format!("{e:#}")}));
                                    let _ = tx.send(Msg::Status(json!({"type": "error",
                                        "error": format!("waiting for microphone ({e:#}) — allow the macOS prompt / connect the mic; retrying")})));
                                    std::thread::sleep(Duration::from_secs(3));
                                }
                            }
                        };
                        // Record the session (16 kHz mono) next to the log so live runs can be replayed
                        // offline: `ls-replay logs/run-….wav`. Header is flushed every second so the
                        // file stays valid even if the app is killed.
                        let wav_path = log.path.with_extension("wav");
                        let mut wav = hound::WavWriter::create(&wav_path, hound::WavSpec {
                            channels: 1, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int,
                        }).ok();
                        log.log(json!({"ev": "mic", "device": name, "recording": wav.as_ref().map(|_| wav_path.display().to_string())}));
                        let _ = tx.send(Msg::Status(json!({"type": "mic", "device": name})));
                        let _ = tx.send(Msg::AudioStart(log.now_ms()));
                        let mut since_flush = 0usize;
                        while !stop.load(Ordering::Relaxed) {
                            if let Ok(block) = arx.recv_timeout(Duration::from_millis(100)) {
                                if let Some(w) = wav.as_mut() {
                                    for &x in &block {
                                        let _ = w.write_sample((x.clamp(-1.0, 1.0) * 32767.0) as i16);
                                    }
                                    since_flush += block.len();
                                    if since_flush >= SR {
                                        let _ = w.flush();
                                        since_flush = 0;
                                    }
                                }
                                emit(ch.push(&block)?);
                            }
                        }
                        if let Some(w) = wav {
                            let _ = w.finalize();
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = r {
                log.log(json!({"ev": "hear_error", "error": format!("{e:#}")}));
                eprintln!("hear error: {e:#}");
                let _ = tx.send(Msg::HearError(format!("{e:#}")));
            }
            let _ = tx.send(Msg::HearDone);
        });
    }

    let mut stage = Stage::new(cfg.stage);
    let mut summary = Summary::default();
    let mut prev_final = String::new();
    let mut audio_t0: u64 = 0;
    let mut chunk_end_wall: std::collections::HashMap<u64, u64> = Default::default();
    let mut hear_done_at: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(50));

    let handle_outcome = |chunk_id: u64, o: Outcome, now: u64, summary: &mut Summary, ends: &std::collections::HashMap<u64, u64>| -> Option<RenderEvent> {
        *summary.outcomes.entry(outcome_name(&o).into()).or_default() += 1;
        match &o {
            Outcome::Rendered(ev) => {
                sink.render(ev);
                let lat = ends.get(&chunk_id).map(|e| now.saturating_sub(*e));
                summary.renders += 1;
                if let Some(l) = lat {
                    summary.render_latencies_ms.push(l);
                }
                summary.shown.push(ev.image_id.clone().unwrap_or_else(|| "(clear)".into()));
                log.log(json!({"ev": "render", "chunk_id": chunk_id, "kind": ev.kind, "image_id": ev.image_id,
                    "speech_to_render_ms": lat}));
                sink.status(&json!({"type": "outcome", "chunk_id": chunk_id, "outcome": "rendered", "image_id": ev.image_id, "latency_ms": lat}));
                Some(ev.clone())
            }
            other => {
                log.log(json!({"ev": "join", "chunk_id": chunk_id, "outcome": outcome_name(other)}));
                if !matches!(other, Outcome::NoChange) {
                    sink.status(&json!({"type": "outcome", "chunk_id": chunk_id, "outcome": outcome_name(other)}));
                }
                None
            }
        }
    };

    // ---- canvas mode state ----
    let mut canvas = Canvas::new();
    let mut chunk_text: std::collections::HashMap<u64, String> = Default::default();
    let mut agent_busy = false;
    let mut agent_next: Option<(String, String, u64)> = None;
    let mut last_curr = String::new();
    // The agent sees the last few finished phrases (a process or a set of numbers spans sentences).
    let mut recent: std::collections::VecDeque<String> = Default::default();
    // A number / structure word was heard; ask the agent once the sentence is complete.
    let mut graphic_pending = false;
    let mut last_clear: Option<Instant> = None;
    let caption_of = |id: &str| -> String {
        engine.searcher.index.entries.iter().find(|e| e.id == id).map(|e| e.caption.clone()).unwrap_or_else(|| id.to_string())
    };
    let spawn_agent = |scene: Scene, prev: String, curr: String, chunk_id: u64| {
        let (e, tx, log) = (engine.clone(), tx.clone(), log.clone());
        tokio::spawn(async move {
            let t = log.now_ms();
            let (ops, src) = e.agent.propose(&scene, &prev, &curr).await;
            let _ = tx.send(Msg::Agent(scene.version, ops, src, chunk_id, log.now_ms() - t));
        });
    };

    loop {
        let mut fresh: Vec<RenderEvent> = vec![];
        let mut agent_trigger: Option<(String, String, u64)> = None;
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                match msg {
                    Msg::AudioStart(t) => {
                        audio_t0 = t;
                        sink.status(&json!({"type": "mic_ok"})); // audio is flowing → "listening"
                    }
                    Msg::HearDone => hear_done_at = Some(Instant::now()),
                    Msg::HearError(e) => sink.status(&json!({"type": "error", "error": e})),
                    Msg::Status(v) => sink.status(&v),
                    Msg::Chunk(c, tm, wall) => {
                        summary.chunks += 1;
                        chunk_end_wall.insert(c.id, audio_t0 + c.t_end_ms);
                        log.log(json!({"ev": "chunk", "chunk": c, "audio_ms": tm.audio_ms, "vad_ms": tm.vad_ms.round(),
                            "asr_ms": tm.asr_ms.round(), "emitted_ms": wall, "asr_lag_ms": wall.saturating_sub(audio_t0 + c.t_end_ms)}));
                        sink.status(&json!({"type": "chunk", "text": c.text, "final": c.is_final}));
                        stage.on_chunk(&c);
                        chunk_text.insert(c.id, c.text.clone());
                        if chunk_text.len() > 256 {
                            let min = *chunk_text.keys().min().unwrap();
                            chunk_text.remove(&min);
                        }
                        // Only words not seen in the previous update of this phrase can carry a new cue.
                        let new_words = c.text.strip_prefix(last_curr.trim_end_matches(|ch: char| !ch.is_alphanumeric())).unwrap_or(&c.text).to_string();
                        last_curr = if c.is_final { String::new() } else { c.text.clone() };
                        let context = recent.iter().cloned().collect::<Vec<_>>().join(" ");
                        if cfg.canvas && !canvas.scene().elements.is_empty() && ls_canvas::has_layout_cue(&new_words) {
                            agent_trigger = Some((context.clone(), c.text.clone(), c.id));
                        }
                        if cfg.canvas && ls_canvas::has_graphic_cue(&new_words) {
                            graphic_pending = true;
                        }
                        // Sentence complete (final chunk, or Whisper closed the sentence) → graphics call.
                        if cfg.canvas && graphic_pending && (c.is_final || c.text.trim_end().ends_with(['.', '?', '!'])) {
                            graphic_pending = false;
                            agent_trigger = Some((context, c.text.clone(), c.id));
                        }
                        if c.is_final {
                            recent.push_back(c.text.clone());
                            while recent.len() > 5 {
                                recent.pop_front();
                            }
                        }
                        let mut displayed = stage.displayed();
                        if cfg.canvas {
                            displayed.on_screen = ls_canvas::board_summary(canvas.scene());
                        }
                        let prev = prev_final.clone();
                        if c.is_final {
                            prev_final = c.text.clone();
                        }
                        // Branch 1: whether.
                        {
                            let (e, tx, c, d, prev, log) = (engine.clone(), tx.clone(), c.clone(), displayed.clone(), prev.clone(), log.clone());
                            tokio::spawn(async move {
                                let t = log.now_ms();
                                let (dec, src) = e.decider.decide(c.id, c.id, &prev, &c.text, &d, t).await;
                                let _ = tx.send(Msg::Decision(dec, src, t, log.now_ms()));
                            });
                        }
                        // Branch 2: what.
                        {
                            let (e, tx, c, d, prev, log) = (engine.clone(), tx.clone(), c.clone(), displayed, prev, log.clone());
                            tokio::spawn(async move {
                                let t = log.now_ms();
                                let q = e.query.query(c.id, &prev, &c.text, &d).await;
                                let tq = log.now_ms();
                                let (e2, phrases, on_screen) = (e.clone(), q.phrases.clone(), d.image_id.clone());
                                let res = tokio::task::spawn_blocking(move || e2.searcher.best_match_avoiding(&e2.clip, c.id, &phrases, on_screen.as_deref())).await;
                                let (best, hits) = match res {
                                    Ok(Ok(v)) => v,
                                    _ => (None, vec![]),
                                };
                                let _ = tx.send(Msg::Search(SearchOutcome { chunk_id: c.id, best }, q, hits, t, tq, log.now_ms()));
                            });
                        }
                    }
                    Msg::Agent(version, mut ops, src, chunk_id, ms) => {
                        agent_busy = false;
                        // One clear per section cue: the partial and the final of "let's move on to one
                        // last thing…" both asked to clear, wiping a photo that had just appeared (09-19).
                        if last_clear.is_some_and(|t: Instant| t.elapsed() < Duration::from_secs(6)) {
                            ops.retain(|o| !matches!(o, ls_canvas::Op::ClearBoard));
                        }
                        if ops.iter().any(|o| matches!(o, ls_canvas::Op::ClearBoard)) {
                            last_clear = Some(Instant::now());
                        }
                        let applied = canvas.apply(version, &ops, chunk_id);
                        log.log(json!({"ev": "agent", "chunk_id": chunk_id, "source": format!("{src:?}"), "ms": ms,
                            "ops": ops, "applied": applied.is_some(), "stale": version != canvas.scene().version && applied.is_none()}));
                        sink.status(&json!({"type": "agent", "chunk_id": chunk_id, "source": format!("{src:?}"), "ms": ms,
                            "ops": ops.len(), "applied": applied.as_ref().map(|s| s.reason.clone())}));
                        if let Some(scene) = applied {
                            let images = || scene.elements.iter().filter(|e| e.kind == ls_canvas::ElementKind::Image);
                            let focused = images().find(|e| e.focus).or_else(|| images().last());
                            stage.sync_current(focused.map(|e| (e.image_id.clone(), e.caption.clone())));
                            log.log(scene_log(&scene));
                            sink.scene(&scene);
                        }
                        if let Some((p, c, id)) = agent_next.take() {
                            agent_busy = true;
                            spawn_agent(canvas.scene().clone(), p, c, id);
                        }
                    }
                    Msg::Decision(d, src, t_start, t_end) => {
                        *summary.decide_sources.entry(format!("{src:?}")).or_default() += 1;
                        log.log(json!({"ev": "decide", "chunk_id": d.chunk_id, "action": d.action, "p": d.p,
                            "source": format!("{src:?}"), "start_ms": t_start, "ms": t_end - t_start}));
                        sink.status(&json!({"type": "decide", "chunk_id": d.chunk_id, "action": d.action, "p": d.p,
                            "source": format!("{src:?}"), "ms": t_end - t_start}));
                        let now = log.now_ms();
                        if let Some(o) = stage.on_decision(d.clone(), now) {
                            fresh.extend(handle_outcome(d.chunk_id, o, now, &mut summary, &chunk_end_wall));
                        }
                    }
                    Msg::Search(s, q, hits, t_start, t_query, t_end) => {
                        if q.from_fallback {
                            summary.query_fallbacks += 1;
                        }
                        log.log(json!({"ev": "search", "chunk_id": s.chunk_id, "phrases": q.phrases, "fallback": q.from_fallback,
                            "query_ms": t_query - t_start, "search_ms": t_end - t_query,
                            "best": s.best.as_ref().map(|m| json!({"id": m.image_id, "score": m.score, "phrase": m.phrase})),
                            "hits": hits.iter().map(|h| json!({"phrase": h.phrase, "id": h.id, "score": h.score})).collect::<Vec<_>>()}));
                        sink.status(&json!({"type": "search", "chunk_id": s.chunk_id, "phrases": q.phrases, "fallback": q.from_fallback,
                            "best": s.best.as_ref().map(|m| m.image_id.clone()), "score": s.best.as_ref().map(|m| m.score),
                            "query_ms": t_query - t_start, "search_ms": t_end - t_query}));
                        let now = log.now_ms();
                        let id = s.chunk_id;
                        if let Some(o) = stage.on_search(s, now) {
                            fresh.extend(handle_outcome(id, o, now, &mut summary, &chunk_end_wall));
                        }
                    }
                }
            }
            _ = tick.tick() => {
                let now = log.now_ms();
                for (id, o) in stage.tick(now) {
                    fresh.extend(handle_outcome(id, o, now, &mut summary, &chunk_end_wall));
                }
                // After the audio ends, drain in-flight work and any pending visual (≤ hold), then stop.
                if let Some(t) = hear_done_at {
                    if t.elapsed() > Duration::from_millis(cfg.stage.hold_render_ms + 1500) {
                        break;
                    }
                }
            }
        }
        // ---- canvas mode: fast path onto the board, then (maybe) the agent ----
        if cfg.canvas {
            for ev in fresh {
                let scene = match (ev.kind, ev.image_id.as_deref()) {
                    ("clear", _) | (_, None) => {
                        last_clear = Some(Instant::now());
                        canvas.clear(ev.chunk_id)
                    }
                    ("update", Some(id)) => canvas.update(id, &caption_of(id), ev.url.as_deref().unwrap_or_default(), ev.chunk_id),
                    (_, Some(id)) => canvas.render(id, &caption_of(id), ev.url.as_deref().unwrap_or_default(), ev.chunk_id),
                };
                log.log(scene_log(&scene));
                sink.scene(&scene);
                if !scene.elements.is_empty() {
                    let text = chunk_text.get(&ev.chunk_id).cloned().unwrap_or_default();
                    agent_trigger = Some((recent.iter().cloned().collect::<Vec<_>>().join(" "), text, ev.chunk_id));
                }
            }
            if let Some(t) = agent_trigger {
                if agent_busy {
                    agent_next = Some(t); // latest wins; runs when the in-flight call returns
                } else {
                    agent_busy = true;
                    spawn_agent(canvas.scene().clone(), t.0, t.1, t.2);
                }
            }
        }
    }
    log.log(json!({"ev": "summary", "summary": &summary, "p50": summary.pct(0.5), "p95": summary.pct(0.95),
        "cache": engine.cache.stats()}));
    Ok(summary)
}
