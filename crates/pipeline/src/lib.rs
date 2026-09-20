//! Orchestration: Hear → transcript → Luna (the canvas agent) → board, with a JSONL log of every step.
//! Luna decides everything that happens on screen: it sees the whole transcript, the board and what it changed
//! recently, and answers with tool calls. Photos are `show_photo` requests: the pipeline embeds Luna's phrase
//! with CLIP, searches the library, and draws the picture (image generation) when the library has nothing.
//! Used by the Tauri app and by `ls-replay`.

use ls_agent::{AgentInput, CanvasAgent, Change, Sentence};
use ls_canvas::{Canvas, ElementKind, Op, Scene};
use ls_contracts::{Chunk, Match, Rect, RenderEvent};
use ls_gen::ImageGen;
use ls_hear::{audio, whisper, ChunkTiming, Chunker, ChunkerConfig, SR};
use ls_search::icons::IconSearch;
use ls_search::{assets, Clip, ImageCache, Index, Searcher, TextEncoder};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
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
    /// Asset card root (LS_ASSETS): `manifest.json`, `embeddings.npy`, `images/`, `icons/`.
    pub assets: Option<PathBuf>,
    /// OpenAI CLIP ViT-B/32 text tower, used with `assets`.
    pub clip_text_dir: PathBuf,
    /// Asset card: how close a photo's class label must be to the query (LABEL_MIN).
    pub label_min: f32,
    /// Asset card: image score an unlabelled (COCO) photo needs instead (UNLABELLED_MIN).
    pub unlabelled_min: f32,
    /// Image generation fallback (BASETEN_API_KEY / BASETEN_URL / GEN_SIZE).
    pub gen_key: Option<String>,
    pub gen_url: Option<String>,
    pub gen_size: Option<u32>,
    pub index_path: PathBuf,
    /// OpenRouter key (`OPENROUTER_API_KEY`); Luna's fallback backend.
    pub api_key: Option<String>,
    /// OpenAI key (`OPENAI_API_KEY`): Luna is called on OpenAI's own API when this is set.
    pub openai_key: Option<String>,
    pub log_path: PathBuf,
    /// Generated images are kept here and reused on the next run (AS13).
    pub gen_dir: PathBuf,
    /// `LS_THEME` (`sketch` default, or `slate`): monochrome icons are recoloured to read on it.
    pub theme: String,
    /// Lowest photo score a library match needs (`TAU`). The label gate does the real rejecting on the asset card.
    pub tau: f32,
    /// Most agent calls per minute (`AGENT_RPM`). Default: the measured 500 max on OpenAI, 18 on OpenRouter.
    pub agent_rpm: Option<u32>,
    pub chunker: ChunkerConfig,
    /// Words the talk uses that Whisper mangles (TALK_TERMS / talk-terms.txt) — passed as its initial prompt.
    pub talk_terms: Vec<String>,
    pub canvas_model: Option<String>,
}

impl Config {
    /// Defaults relative to the repo root; `.env` / env vars override the API settings.
    pub fn from_env(root: &std::path::Path) -> Self {
        let _ = dotenvy::from_path(root.join(".env"));
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let p = |k: &str, d: &str| env(k).map(PathBuf::from).unwrap_or_else(|| root.join(d));
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        // MobileCLIP cosines sit around 0.52 for a real match; CLIP ViT-B/32 on the asset card far lower
        // (labelled hits 0.23–0.32, a broad library scores ≈ 0.29 for anything), so there τ is only a floor and
        // the label gate does the real rejecting.
        let tau = env("TAU").and_then(|v| v.parse().ok()).unwrap_or(if env("LS_ASSETS").is_some() { 0.22 } else { 0.52 });
        Self {
            // small.en: 16% WER / 10 of 10 keywords on the noisy fixture vs base.en 46% / 4 of 10, at
            // 423 ms p50 with the audio-context floor (PROGRESS 09-19). Falls back to base.en if absent.
            assets: env("LS_ASSETS").map(PathBuf::from),
            clip_text_dir: p("CLIP_TEXT_DIR", "models/clip-vit-b32"),
            // 0.92: measured on the card — "a rose"→rose 0.93, while owl→bird 0.87 and junk 0.81–0.84.
            gen_dir: root.join("generated"),
            gen_key: env("BASETEN_API_KEY"),
            gen_url: env("BASETEN_URL"),
            gen_size: env("GEN_SIZE").and_then(|v| v.parse().ok()),
            label_min: env("LABEL_MIN").and_then(|v| v.parse().ok()).unwrap_or(0.92),
            // Off by default: the card's 5k unlabelled COCO photos score like real matches for anything
            // ("night hunting" → a random photo at 0.32), and nothing separates them. UNLABELLED_MIN=0.31
            // re-enables them if recall matters more than precision.
            unlabelled_min: env("UNLABELLED_MIN").and_then(|v| v.parse().ok()).unwrap_or(f32::INFINITY),
            whisper_model: p("WHISPER_MODEL", if root.join("models/ggml-small.en.bin").exists() { "models/ggml-small.en.bin" } else { "models/ggml-base.en.bin" }),
            vad_model: p("VAD_MODEL", "models/ggml-silero-v5.1.2.bin"),
            clip_dir: p("CLIP_DIR", "models/mobileclip-s2"),
            index_path: p("INDEX", "dev-library/index.json"),
            api_key: env("OPENROUTER_API_KEY"),
            openai_key: env("OPENAI_API_KEY"),
            log_path: root.join("logs").join(format!("run-{stamp}.jsonl")),
            theme: env("LS_THEME").unwrap_or_else(|| "sketch".into()),
            tau,
            agent_rpm: env("AGENT_RPM").and_then(|v| v.parse().ok()),
            chunker: {
                let mut c = ChunkerConfig::default();
                c.tick_ms = 600; // small.en p90 ≈ 640 ms per pass; a 500 ms tick would fall behind
                if let Some(t) = env("ASR_TICK_MS").and_then(|v| v.parse().ok()) {
                    c.tick_ms = t;
                }
                c
            },
            talk_terms: env("TALK_TERMS")
                .or_else(|| std::fs::read_to_string(root.join("talk-terms.txt")).ok())
                .map(|v| v.split([',', '\n']).map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && !t.starts_with('#')).collect())
                .unwrap_or_default(),
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
    pub clip: Arc<dyn TextEncoder>,
    pub searcher: Arc<Searcher>,
    pub cache: ImageCache,
    pub agent: CanvasAgent,
    pub gen: ImageGen,
    /// Distinct library subjects, for the offline fallback (no key / Luna down).
    pub vocab: Vec<String>,
    /// Logos, icons and flags (`LS_ASSETS/icons`): searched by name and alias, not by embedding.
    pub icons: Option<Arc<IconSearch>>,
}

impl Engine {
    pub async fn load(cfg: Config) -> anyhow::Result<Self> {
        // LS_ASSETS → Marcus's card (CLIP ViT-B/32 text tower over precomputed image embeddings);
        // otherwise the locally built MobileCLIP index. The two spaces must never be mixed.
        let (clip, index): (Arc<dyn TextEncoder>, Index) = match &cfg.assets {
            Some(dir) => (Arc::new(assets::ClipText::load(&cfg.clip_text_dir)?), assets::load_index(dir)?),
            None => (Arc::new(Clip::load(&cfg.clip_dir)?), Index::load(&cfg.index_path)?),
        };
        let vocab = index.vocab(200);
        let cache = ImageCache::new(&index, 256 * 1024 * 1024);
        let mut searcher = Searcher::new(index);
        if cfg.assets.is_some() {
            // Broad library → a photo only shows when its own label is about the query (see LabelGate).
            let cache_path = cfg.clip_text_dir.join("label-vectors.json");
            searcher = searcher.with_label_gate(&*clip, &cache_path, cfg.label_min, cfg.unlabelled_min)?;
        }
        let searcher = Arc::new(searcher);
        let http = reqwest::Client::builder().pool_idle_timeout(Duration::from_secs(300)).tcp_keepalive(Duration::from_secs(30)).build()?;
        let agent = CanvasAgent::new(http.clone(), cfg.api_key.clone(), cfg.canvas_model.clone()).with_openai(cfg.openai_key.clone());
        let gen = ImageGen::new(http, cfg.gen_key.clone(), cfg.gen_url.clone(), cfg.gen_size);
        let icons = cfg.assets.as_ref().map(|d| d.join("icons")).filter(|d| d.join("lookup.json").exists()).and_then(|d| match IconSearch::load(&d) {
            Ok(i) => Some(Arc::new(i)),
            Err(e) => {
                eprintln!("symbol library not loaded ({}): {e:#}", d.display());
                None
            }
        });
        Ok(Self { cfg, clip, searcher, cache, agent, gen, vocab, icons })
    }

    /// Warm-up: CLIP text path, image prefetch, Luna's Responses socket, and the image-generation deployment.
    pub async fn warm_up(&self, log: &Logger) {
        let t = Instant::now();
        let _ = self.clip.embed_text("warm up");
        // Prefetch only a small curated library; the asset card holds ~15k photos (LRU on demand).
        let n = if self.searcher.index.entries.len() <= 256 { self.cache.prefetch_all() } else { 0 };
        if self.gen.enabled() {
            // The deployment scales to zero (146 s cold); wake it without blocking startup.
            let g = self.gen.clone();
            tokio::spawn(async move { g.warm_up().await });
        }
        let luna_ws = match self.agent.warm_up().await {
            Ok(ms) => json!({"enabled": ms.is_some(), "ms": ms}),
            Err(e) => json!({"enabled": true, "error": format!("{e:#}")}),
        };
        log.log(json!({"ev": "warm_up", "ms": t.elapsed().as_millis() as u64, "prefetched": n, "remote": self.agent.has_remote(), "symbols": self.icons.is_some(), "luna_ws": luna_ws}));
    }
}

enum Msg {
    Chunk(Chunk, ChunkTiming, u64 /* wall ms when emitted */),
    AudioStart(u64),
    HearDone,
    HearError(String),
    Status(Value),
    /// Luna answered.
    Agent { seen: Scene, proposal: ls_agent::Proposal, call: u64, chunk_id: u64, ms: u64, heard_at: u64 },
    /// A photo search for a `show_photo` request finished.
    Photo { subject: String, replace: bool, chunk_id: u64, heard_at: u64, best: Option<Match>, search_ms: u64 },
    /// A generated image arrived (None = failed or refused).
    Generated { subject: String, replace: bool, chunk_id: u64, heard_at: u64, bytes: Option<Vec<u8>>, ms: u64, asked: Instant },
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Summary {
    pub chunks: usize,
    /// Photos placed on the board.
    pub renders: usize,
    /// Speech → photo on screen, per photo.
    pub render_latencies_ms: Vec<u64>,
    pub agent_calls: usize,
    /// Calls where the model failed and the offline rules answered.
    pub agent_fallbacks: usize,
    pub ops_applied: usize,
    pub ops_refused: usize,
    /// Photos drawn by the generation fallback because the library had nothing.
    pub generated: usize,
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

/// Do two subjects describe the same picture? (Token overlap, ignoring filler words.)
fn similar(a: &str, b: &str) -> bool {
    let words = |s: &str| -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2 && !["the", "and", "from", "with", "into", "our", "for"].contains(w))
            .map(|w| w.trim_end_matches('s').to_string())
            .collect()
    };
    let (x, y) = (words(a), words(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let shared = x.intersection(&y).count();
    shared * 2 >= x.len().min(y.len()) * 2 // every word of the shorter subject appears in the longer one
}

/// Filename-safe form of a spoken subject ("a sunflower in a field" → "a-sunflower-in-a-field").
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(60).collect()
}

/// Compact scene line for the run log (eval + debugging): tile kinds and graphic contents.
fn scene_log(scene: &Scene) -> Value {
    let tiles: Vec<Value> = scene
        .elements
        .iter()
        .map(|e| match (&e.diagram, &e.chart, &e.text) {
            (Some(d), _, _) => json!({"id": e.id, "kind": "diagram", "layout": d.layout, "title": d.title,
                "nodes": d.nodes.iter().map(|n| n.label.clone()).collect::<Vec<_>>(), "edges": d.edges.len(),
                "pictures": d.nodes.iter().filter(|n| n.icon.is_some()).count()}),
            (_, Some(c), _) => json!({"id": e.id, "kind": "chart", "chart": c.kind, "title": c.title, "unit": c.unit,
                "points": c.points.iter().map(|p| json!([p.label, p.value])).collect::<Vec<_>>(), "pictures": c.points.iter().filter(|p| p.icon.is_some()).count()}),
            (_, _, Some(t)) => json!({"id": e.id, "kind": "text", "blocks": t.blocks.iter().map(|b| json!({"id": b.id, "kind": b.kind, "text": b.text})).collect::<Vec<_>>()}),
            _ if e.kind == ElementKind::Logo => json!({"id": e.id, "kind": "logo", "asset": e.image_id, "title": e.caption}),
            _ => json!({"id": e.id, "kind": "image", "image_id": e.image_id}),
        })
        .collect();
    json!({"ev": "scene", "version": scene.version, "reason": scene.reason, "n": scene.elements.len(), "layout": scene.layout, "tiles": tiles})
}


/// Words in `curr` beyond what `prev` already had (a growing partial repeats the words it started with).
fn new_words_in(curr: &str, prev: &str) -> usize {
    let p = prev.trim_end_matches(|c: char| !c.is_alphanumeric());
    let tail = if !p.is_empty() && curr.starts_with(p) { &curr[p.len()..] } else { curr };
    word_count(tail)
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

fn push_change(changes: &mut VecDeque<Change>, at_s: u64, what: String) {
    changes.push_back(Change { at_s, what });
    while changes.len() > 10 {
        changes.pop_front();
    }
}

/// No key or Luna down: the old presenter-cue heuristic. A cue ("here's…", "take a look at…") followed by a
/// library subject shows that subject; anything else shows nothing.
fn offline_photo(text: &str, vocab: &[String]) -> Option<String> {
    let after = ls_query::after_last_cue(text, 10)?;
    ls_query::named_subject(&after.join(" "), vocab)
}

/// A `show_photo` subject that is really a symbol request ("Google logo", "an icon for teamwork"): the tool the model
/// should have used. Diffusion models draw these badly and the CLIP search cannot find them.
fn symbol_tool(subject: &str) -> Option<&'static str> {
    let words: Vec<String> = subject.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect();
    let has = |ws: &[&str]| words.iter().any(|w| ws.contains(&w.as_str()));
    if has(&["logo", "logos", "wordmark", "brand"]) {
        Some("show_logo")
    } else if has(&["icon", "icons"]) {
        Some("show_icon")
    } else {
        None
    }
}

/// Monochrome icons draw in `currentColor`, which is black inside an `<img>`: write a copy in the theme's ink.
fn themed_copy(src: &std::path::Path, dir: &std::path::Path, id: &str, theme: &str) -> Option<PathBuf> {
    let out = dir.join("symbols").join(format!("{}-{theme}.svg", slug(id)));
    if !out.exists() {
        let svg = std::fs::read_to_string(src).ok()?;
        let ink = if theme == "slate" { "#f1f3f7" } else { "#2b2723" };
        std::fs::create_dir_all(out.parent()?).ok()?;
        std::fs::write(&out, svg.replace("currentColor", ink)).ok()?;
    }
    Some(out)
}

/// Make a symbol reachable by the web view: register its file with the image cache (monochrome icons in the theme's ink)
/// and return its title and `img://` url.
fn symbol_asset(engine: &Engine, id: &str) -> Option<(String, String)> {
    let icons = engine.icons.as_ref()?;
    let (path, entry) = (icons.path_of(id)?, icons.entry(id)?);
    let served = if entry.monochrome { themed_copy(&path, &engine.cfg.gen_dir, id, &engine.cfg.theme).unwrap_or(path) } else { path };
    let cache_id = format!("{}.svg", slug(id));
    engine.cache.add(&cache_id, served);
    Some((entry.title.clone(), format!("img://localhost/{cache_id}")))
}

/// Turn the model's `logo` / `icon` hints on diagram nodes and chart points into picture urls, before the ops reach
/// the canvas. Pictures appear on most nodes of a diagram or on none (an uneven scatter looks like a mistake), added nodes
/// match the diagram they join, and a hint that finds nothing exact is dropped.
fn attach_pictures(engine: &Engine, canvas: &Canvas, ops: &mut [Op], log: &Logger) {
    let Some(icons) = engine.icons.as_ref() else {
        // no symbol library: never leave a raw hint word in a node (it would be drawn as an emoji)
        for op in ops.iter_mut() {
            match op {
                Op::DrawDiagram { nodes, .. } | Op::AddNodes { nodes, .. } => nodes.iter_mut().for_each(|n| { n.icon = None; n.logo = None; }),
                Op::DrawChart { points, .. } => points.iter_mut().for_each(|p| { p.icon = None; p.logo = None; }),
                Op::AddPoint { icon, logo, .. } => { *icon = None; *logo = None; }
                _ => {}
            }
        }
        return;
    };
    let url = |hit: Option<ls_search::icons::Hit>| hit.and_then(|h| symbol_asset(engine, &h.id).map(|(_, u)| u));
    let has_pictures = |id: &str, node: bool| {
        canvas.scene().elements.iter().find(|e| e.id == id).is_some_and(|e| if node { e.diagram.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.icon.is_some())) } else { e.chart.as_ref().is_some_and(|c| c.points.iter().any(|p| p.icon.is_some())) })
    };
    for op in ops.iter_mut() {
        match op {
            Op::DrawDiagram { nodes, .. } => {
                let found: Vec<Option<String>> = nodes.iter().map(|n| url(icons.picture_for(n.logo.as_deref(), n.icon.as_deref(), &n.label, true))).collect();
                let keep = !nodes.is_empty() && found.iter().filter(|f| f.is_some()).count() * 10 >= nodes.len() * 6;
                log.log(json!({"ev": "pictures", "op": "draw_diagram", "kept": keep, "nodes": nodes.iter().zip(&found).map(|(n, f)| json!([n.label, n.logo, n.icon, f.is_some()])).collect::<Vec<_>>()}));
                for (n, f) in nodes.iter_mut().zip(found) {
                    n.icon = if keep { f } else { None };
                    n.logo = None;
                }
            }
            Op::AddNodes { id, nodes, .. } => {
                let join = has_pictures(id, true);
                for n in nodes.iter_mut() {
                    let found = if join { url(icons.picture_for(n.logo.as_deref(), n.icon.as_deref(), &n.label, true)) } else { None };
                    log.log(json!({"ev": "pictures", "op": "add_nodes", "label": n.label, "logo": n.logo, "icon": n.icon, "diagram_has_pictures": join, "found": found.is_some()}));
                    n.icon = found;
                    n.logo = None;
                }
            }
            // charts: only a named product gets a logo; the label is never turned into an icon
            Op::DrawChart { points, .. } => {
                let found: Vec<Option<String>> = points.iter().map(|p| url(icons.picture_for(p.logo.as_deref(), None, &p.label, false))).collect();
                let keep = !points.is_empty() && found.iter().filter(|f| f.is_some()).count() * 10 >= points.len() * 6;
                log.log(json!({"ev": "pictures", "op": "draw_chart", "kept": keep, "points": points.iter().zip(&found).map(|(p, f)| json!([p.label, p.logo, f.is_some()])).collect::<Vec<_>>()}));
                for (p, f) in points.iter_mut().zip(found) {
                    p.icon = if keep { f } else { None };
                    p.logo = None;
                }
            }
            Op::AddPoint { id, label, icon, logo, .. } => {
                let join = has_pictures(id, false);
                // Joining a chart whose other points carry logos, the point's own name is the brand when the model gave no hint.
                *icon = if join { url(icons.picture_for(logo.as_deref().or(Some(label.as_str())), None, label, false)) } else { None };
                log.log(json!({"ev": "pictures", "op": "add_point", "label": label, "logo": logo, "chart_has_pictures": join, "found": icon.is_some()}));
                *logo = None;
            }
            _ => {}
        }
    }
}

/// Resolve a logo / icon / flag request against the symbol library and put the result on the board. When there is no
/// library, or nothing in it fits, a plain card with the asked-for name goes up instead: the screen is never left
/// blank and nothing is invented. `queries[0]` is what was asked for; the rest are the model's synonyms.
#[allow(clippy::too_many_arguments)]
fn show_symbol(engine: &Engine, canvas: &mut Canvas, sink: &dyn RenderSink, log: &Logger, summary: &mut Summary, changes: &mut VecDeque<Change>, tool: &str, queries: &[String], replace: bool, call: u64, chunk_id: u64, heard_at: u64) {
    let asked = queries.first().map(|q| q.trim().to_string()).unwrap_or_default();
    let kinds: &[&str] = if tool == "show_logo" { &["logo"] } else { &["icon", "flag"] };
    let (pick, ranked) = engine.icons.as_ref().map_or((None, vec![]), |i| i.resolve(kinds, queries));
    let resolved = pick.as_ref().and_then(|h| symbol_asset(engine, &h.id).map(|(title, url)| (h.id.clone(), title, url)));
    let (asset, title, url) = resolved.clone().unwrap_or((String::new(), asked.clone(), String::new()));
    let scene = if replace { canvas.replace_logo(&asset, &title, &url, chunk_id) } else { canvas.render_logo(&asset, &title, &url, chunk_id) };
    let now = log.now_ms();
    let lat = now.saturating_sub(heard_at);
    log.log(json!({"ev": "symbol", "call": call, "tool": tool, "queries": queries, "library": engine.icons.is_some(),
        "pick": pick.as_ref().map(|h| json!({"id": h.id, "score": h.score, "how": h.how})),
        "candidates": ranked.iter().take(3).map(|h| json!([h.id, h.score, h.how])).collect::<Vec<_>>(), "name_card": resolved.is_none()}));
    summary.renders += 1;
    summary.render_latencies_ms.push(lat);
    summary.shown.push(if asset.is_empty() { format!("card:{title}") } else { asset.clone() });
    log.log(json!({"ev": "render", "chunk_id": chunk_id, "kind": "logo", "image_id": asset, "how": if resolved.is_some() { "symbol library" } else { "name card" }, "speech_to_render_ms": lat}));
    log.log(scene_log(&scene));
    sink.scene(&scene);
    sink.status(&json!({"type": "outcome", "chunk_id": chunk_id, "outcome": "rendered", "image_id": if asset.is_empty() { title.clone() } else { asset.clone() }, "latency_ms": lat}));
    push_change(changes, now / 1000, format!("{tool} {title}{}", if resolved.is_some() { "" } else { " (name card: not in the library)" }));
}

/// Put a photo on the board (a new tile, or swapping the focused photo) and log and announce it.
#[allow(clippy::too_many_arguments)]
fn show_photo(canvas: &mut Canvas, sink: &dyn RenderSink, log: &Logger, summary: &mut Summary, changes: &mut VecDeque<Change>, replace: bool, image_id: &str, caption: &str, chunk_id: u64, heard_at: u64, how: &str) {
    let url = format!("img://localhost/{image_id}");
    let swap = replace && canvas.scene().elements.iter().any(|e| e.focus && e.kind == ElementKind::Image);
    let (scene, kind) = if swap { (canvas.update(image_id, caption, &url, chunk_id), "update") } else { (canvas.render(image_id, caption, &url, chunk_id), "render") };
    let now = log.now_ms();
    let lat = now.saturating_sub(heard_at);
    summary.renders += 1;
    summary.render_latencies_ms.push(lat);
    summary.shown.push(image_id.to_string());
    log.log(json!({"ev": "render", "chunk_id": chunk_id, "kind": kind, "image_id": image_id, "how": how, "speech_to_render_ms": lat}));
    log.log(scene_log(&scene));
    sink.render(&RenderEvent { kind, image_id: Some(image_id.to_string()), url: Some(url), rect: Rect::FULL, chunk_id, ts_ms: now });
    sink.scene(&scene);
    sink.status(&json!({"type": "outcome", "chunk_id": chunk_id, "outcome": "rendered", "image_id": image_id, "latency_ms": lat}));
    push_change(changes, now / 1000, format!("show_photo {caption} ({how})"));
}

/// Run one talk until the audio ends (WAV) or `stop` is set (mic).
pub async fn run(engine: Arc<Engine>, source: AudioSource, sink: Arc<dyn RenderSink>, log: Logger, stop: Arc<AtomicBool>) -> anyhow::Result<Summary> {
    let cfg = engine.cfg.clone();
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

    // Whisper vocabulary hint: the talk's own words (product and technical terms it otherwise mangles —
    // "prototype" → "ProSive", "text" → "SEX"). From TALK_TERMS or talk-terms.txt; empty = no hint.
    // WHISPER_VOCAB=1 adds the library subjects too (they leaked into noisy audio on 09-19, so opt-in).
    let mut vocab: Vec<String> = cfg.talk_terms.clone();
    if std::env::var("WHISPER_VOCAB").as_deref() == Ok("1") {
        vocab.extend(engine.searcher.index.entries.iter().map(|e| e.caption.split('(').next().unwrap_or(&e.caption).trim().to_string()));
    }
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

    let mut summary = Summary::default();
    let mut audio_t0: u64 = 0;
    let mut hear_done_at: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(50));

    // ---- what Luna is shown ----
    let mut canvas = Canvas::new();
    let mut transcript: Vec<Sentence> = vec![];
    let mut speaking_now = String::new();
    let mut seen_upto = 0usize; // transcript[..seen_upto] went to Luna in an earlier call
    let mut last_speaking = String::new(); // the phrase in progress as of the last call
    let mut changes: VecDeque<Change> = VecDeque::new();
    // ---- when it is called ----
    let rpm = cfg.agent_rpm.unwrap_or_else(|| engine.agent.default_rpm());
    let min_gap = if engine.agent.has_remote() { Duration::from_millis(60_000 / rpm.max(1) as u64) } else { Duration::ZERO };
    let mut agent_busy = false;
    let mut last_call: Option<Instant> = None;
    let mut call_no = 0u64;
    let mut last_chunk_id = 0u64;
    let mut dirty_since: Option<u64> = None; // when words Luna has not seen first arrived
    // ---- photos ----
    let mut last_clear: Option<Instant> = None;
    let mut inflight = 0usize; // photo searches + generations still running
    let mut generating: HashSet<String> = HashSet::new();
    let mut recent_photos: Vec<(String, Instant)> = vec![];
    let caption_of = |id: &str| -> String {
        // Card photos from COCO have no label at all; "photo" keeps board summaries readable.
        match engine.searcher.index.entries.iter().find(|e| e.id == id) {
            Some(e) if !e.caption.trim().is_empty() => e.caption.split('(').next().unwrap_or(&e.caption).trim().to_string(),
            Some(_) => "photo".into(),
            None => id.to_string(),
        }
    };

    loop {
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
                        log.log(json!({"ev": "chunk", "chunk": c, "audio_ms": tm.audio_ms,
                            "speech_rms": (tm.speech_rms * 1000.0).round() / 1000.0, "vad_ms": tm.vad_ms.round(),
                            "asr_ms": tm.asr_ms.round(), "emitted_ms": wall, "asr_lag_ms": wall.saturating_sub(audio_t0 + c.t_end_ms)}));
                        sink.status(&json!({"type": "chunk", "text": c.text, "final": c.is_final}));
                        last_chunk_id = c.id;
                        if c.is_final {
                            let t = c.text.trim();
                            if !t.is_empty() {
                                transcript.push(Sentence { at_s: log.now_ms() / 1000, text: t.to_string() });
                            }
                            speaking_now.clear();
                        } else {
                            speaking_now = c.text.clone();
                        }
                    }
                    Msg::Agent { seen, proposal, call, chunk_id, ms, heard_at } => {
                        agent_busy = false;
                        let now = log.now_ms();
                        summary.agent_calls += 1;
                        if proposal.source == ls_agent::Source::Rules {
                            summary.agent_fallbacks += 1;
                        }
                        let ls_agent::Proposal { mut ops, source, mut dropped, error, transport, first_event_ms, service_tier } = proposal;
                        // One clear per section cue: the partial and the final of "let's move on…" both asked
                        // to clear, wiping a photo that had just appeared (09-19).
                        if last_clear.is_some_and(|t: Instant| t.elapsed() < Duration::from_secs(6)) && ops.iter().any(|o| matches!(o, Op::ClearBoard { .. })) {
                            ops.retain(|o| !matches!(o, Op::ClearBoard { .. }));
                            dropped.push("clear_board: the board was cleared less than 6 s ago".into());
                        }
                        attach_pictures(&engine, &canvas, &mut ops, &log);
                        let no_action: Vec<String> = ops.iter().filter_map(|o| if let Op::NoAction { reason } = o { Some(reason.clone()) } else { None }).collect();
                        let photos: Vec<(String, bool)> = ops.iter().filter_map(|o| if let Op::ShowPhoto { subject, replace } = o { Some((subject.clone(), *replace)) } else { None }).collect();
                        let logos: Vec<(String, bool)> = ops.iter().filter_map(|o| if let Op::ShowLogo { name, replace } = o { Some((name.clone(), *replace)) } else { None }).collect();
                        let icons: Vec<(Vec<String>, bool)> = ops.iter().filter_map(|o| if let Op::ShowIcon { concept, alternatives, replace } = o { Some((std::iter::once(concept.clone()).chain(alternatives.iter().cloned()).collect(), *replace)) } else { None }).collect();
                        let board_ops: Vec<Op> = ops.iter().filter(|o| !matches!(o, Op::NoAction { .. } | Op::ShowPhoto { .. } | Op::ShowLogo { .. } | Op::ShowIcon { .. })).cloned().collect();
                        // Ops address tiles by id, so they apply even though photos may have landed meanwhile;
                        // a clear removes only what Luna saw.
                        let applied = canvas.apply_seen(&seen, &board_ops, chunk_id);
                        let names = canvas.take_applied();
                        let mut refused = dropped;
                        refused.extend(canvas.take_notes());
                        if names.iter().any(|n| n.starts_with("clear_board")) {
                            last_clear = Some(Instant::now());
                        }
                        let mut acknowledged = names.clone();
                        acknowledged.extend(photos.iter().map(|(subject, _)| format!("show_photo {subject}: search queued")));
                        acknowledged.extend(logos.iter().map(|(name, _)| format!("show_logo {name}: placed (or a name card if not in the library)")));
                        acknowledged.extend(icons.iter().map(|(q, _)| format!("show_icon {}: placed (or a name card if nothing fits)", q[0])));
                        engine.agent.acknowledge(&acknowledged, &refused, canvas.scene()).await;
                        summary.ops_applied += names.len();
                        summary.ops_refused += refused.len();
                        log.log(json!({"ev": "agent", "call": call, "chunk_id": chunk_id, "source": format!("{source:?}"), "transport": format!("{transport:?}"), "service_tier": service_tier, "first_event_ms": first_event_ms, "ms": ms, "error": error,
                            "ops": ops, "applied": names, "refused": refused, "no_action": no_action}));
                        sink.status(&json!({"type": "agent", "call": call, "chunk_id": chunk_id, "source": format!("{source:?}"), "transport": format!("{transport:?}"), "service_tier": service_tier, "first_event_ms": first_event_ms, "ms": ms,
                            "ops": ops.len(), "applied": names, "refused": refused, "no_action": no_action,
                            "photos": photos.iter().map(|p| p.0.clone()).chain(logos.iter().map(|n| format!("logo {}", n.0))).chain(icons.iter().map(|q| format!("icon {}", q.0[0]))).collect::<Vec<_>>()}));
                        for n in names {
                            push_change(&mut changes, now / 1000, n);
                        }
                        if let Some(scene) = applied {
                            log.log(scene_log(&scene));
                            sink.scene(&scene);
                        }
                        // Logos and icons are looked up by name (no embeddings, no model): instant, and they never go to generation.
                        for (name, replace) in logos {
                            show_symbol(&engine, &mut canvas, &*sink, &log, &mut summary, &mut changes, "show_logo", &[name], replace, call, chunk_id, heard_at);
                        }
                        for (queries, replace) in icons {
                            show_symbol(&engine, &mut canvas, &*sink, &log, &mut summary, &mut changes, "show_icon", &queries, replace, call, chunk_id, heard_at);
                        }
                        // show_photo: search the library now; a gap goes on to generation when the search lands.
                        for (subject, replace) in photos {
                            // A symbol asked for as a photo ("Google logo"): it belongs in the symbol library.
                            if let Some(tool) = symbol_tool(&subject) {
                                log.log(json!({"ev": "photo_rerouted", "call": call, "subject": &subject, "to": tool}));
                                show_symbol(&engine, &mut canvas, &*sink, &log, &mut summary, &mut changes, tool, &[subject], false, call, chunk_id, heard_at);
                                continue;
                            }
                            // The same subject asked for again within 25 s ("planet Earth" then "planet Earth from
                            // space") is the same picture. A replace is a different request.
                            if !replace && recent_photos.iter().any(|(s, t): &(String, Instant)| t.elapsed() < Duration::from_secs(25) && similar(s, &subject)) {
                                log.log(json!({"ev": "photo_skipped", "call": call, "subject": &subject, "reason": "asked for a moment ago"}));
                                continue;
                            }
                            recent_photos.push((subject.clone(), Instant::now()));
                            recent_photos.retain(|(_, t)| t.elapsed() < Duration::from_secs(60));
                            inflight += 1;
                            let (e, tx, log2) = (engine.clone(), tx.clone(), log.clone());
                            tokio::spawn(async move {
                                let t = log2.now_ms();
                                let (e2, s2) = (e.clone(), subject.clone());
                                let res = tokio::task::spawn_blocking(move || e2.searcher.best_match(&*e2.clip, chunk_id, &[s2]).map(|(m, _)| m)).await;
                                let best = match res {
                                    Ok(Ok(m)) => m,
                                    _ => None,
                                };
                                let _ = tx.send(Msg::Photo { subject, replace, chunk_id, heard_at, best, search_ms: log2.now_ms() - t });
                            });
                        }
                        let _ = seen;
                    }
                    Msg::Photo { subject, replace, chunk_id, heard_at, best, search_ms } => {
                        inflight -= 1;
                        log.log(json!({"ev": "photo_search", "chunk_id": chunk_id, "subject": &subject, "search_ms": search_ms,
                            "best": best.as_ref().map(|m| json!({"id": m.image_id, "score": m.score, "caption": m.caption}))}));
                        match best.filter(|m| m.score >= cfg.tau) {
                            Some(m) => {
                                let caption = caption_of(&m.image_id);
                                show_photo(&mut canvas, &*sink, &log, &mut summary, &mut changes, replace, &m.image_id, &caption, chunk_id, heard_at, "library");
                            }
                            None => {
                                // Nothing in the library is about this subject → draw it (handoff AS12).
                                sink.status(&json!({"type": "outcome", "chunk_id": chunk_id, "outcome": "library_gap"}));
                                // Luna names the subject itself, so a short one ("owl") is a real subject; only what a
                                // diffusion model draws badly (logos, text, vague words) is refused.
                                if !engine.gen.enabled() || ImageGen::refuses(&subject) || subject.trim().is_empty() {
                                    let why = if !engine.gen.enabled() { "generation is off (no BASETEN_API_KEY)" } else { "refused: a logo, text or too vague to draw" };
                                    log.log(json!({"ev": "gap", "chunk_id": chunk_id, "subject": &subject, "generation": why}));
                                } else if generating.insert(subject.clone()) {
                                    inflight += 1;
                                    let cached = cfg.gen_dir.join(format!("{}.jpg", slug(&subject)));
                                    let (e, tx, log2) = (engine.clone(), tx.clone(), log.clone());
                                    tokio::spawn(async move {
                                        let asked = Instant::now();
                                        if cached.exists() {
                                            // generated earlier in this talk (or a rehearsal): reuse, no call
                                            let _ = tx.send(Msg::Generated { subject, replace, chunk_id, heard_at, bytes: None, ms: 0, asked });
                                            return;
                                        }
                                        let t = log2.now_ms();
                                        let bytes = e.gen.generate(&subject).await;
                                        let _ = tx.send(Msg::Generated { subject, replace, chunk_id, heard_at, bytes, ms: log2.now_ms() - t, asked });
                                    });
                                }
                            }
                        }
                    }
                    Msg::Generated { subject, replace, chunk_id, heard_at, bytes, ms, asked } => {
                        inflight -= 1;
                        generating.remove(&subject);
                        let path = cfg.gen_dir.join(format!("{}.jpg", slug(&subject)));
                        if let Some(b) = bytes {
                            let _ = std::fs::create_dir_all(&cfg.gen_dir);
                            let _ = std::fs::write(&path, &b);
                        }
                        // Show it only if the subject is still in what was actually said: a mis-transcribed
                        // partial ("a single scene") is gone by the time its picture lands (09-19).
                        let said: String = transcript.iter().rev().take(6).map(|s| s.text.as_str()).chain(std::iter::once(speaking_now.as_str())).collect::<Vec<_>>().join(" ").to_lowercase();
                        let still_said = subject.to_lowercase().split_whitespace().filter(|w| w.len() > 3).all(|w| said.contains(w.trim_end_matches('s')));
                        let fresh_enough = asked.elapsed() < Duration::from_secs(15);
                        log.log(json!({"ev": "generated", "chunk_id": chunk_id, "subject": &subject, "ms": ms, "ok": path.exists(), "used": path.exists() && still_said && fresh_enough}));
                        if path.exists() && still_said && fresh_enough {
                            let id = format!("gen-{}", slug(&subject));
                            engine.cache.add(&id, path);
                            summary.generated += 1;
                            sink.status(&json!({"type": "generated", "chunk_id": chunk_id, "subject": subject, "ms": ms}));
                            show_photo(&mut canvas, &*sink, &log, &mut summary, &mut changes, replace, &id, &subject, chunk_id, heard_at, "drawn");
                        }
                    }
                }
            }
            _ = tick.tick() => {}
        }

        // ---- ask Luna when there are words it has not seen, it is idle, and the rate cap allows ----
        let fresh_words = transcript[seen_upto..].iter().map(|s| word_count(&s.text)).sum::<usize>() + new_words_in(&speaking_now, &last_speaking);
        if fresh_words > 0 && dirty_since.is_none() {
            dirty_since = Some(log.now_ms());
        }
        let draining = hear_done_at.is_some();
        let due = last_call.is_none_or(|t| t.elapsed() >= min_gap);
        if !agent_busy && due && fresh_words >= if draining { 1 } else { 3 } {
            call_no += 1;
            agent_busy = true;
            last_call = Some(Instant::now());
            let heard_at = dirty_since.take().unwrap_or_else(|| log.now_ms());
            let (e, tx, log2) = (engine.clone(), tx.clone(), log.clone());
            let (seen, tr, new_from, speaking, ch, now_s, call, chunk_id) =
                (canvas.scene().clone(), transcript.clone(), seen_upto, speaking_now.clone(), changes.iter().cloned().collect::<Vec<_>>(), log.now_ms() / 1000, call_no, last_chunk_id);
            log.log(json!({"ev": "agent_call", "call": call, "chunk_id": chunk_id, "sentences": tr.len(), "new_sentences": tr.len() - new_from,
                "speaking_words": word_count(&speaking), "tiles": seen.elements.len()}));
            seen_upto = transcript.len();
            last_speaking = speaking_now.clone();
            tokio::spawn(async move {
                let t = log2.now_ms();
                let input = AgentInput { scene: &seen, transcript: &tr, new_from, speaking_now: &speaking, changes: &ch, now_s };
                let mut proposal = e.agent.propose(&input).await;
                if proposal.source == ls_agent::Source::Rules {
                    if let Some(subject) = offline_photo(&input.newest_text(), &e.vocab) {
                        proposal.ops.push(Op::ShowPhoto { subject, replace: false });
                    }
                }
                let _ = tx.send(Msg::Agent { seen, proposal, call, chunk_id, ms: log2.now_ms() - t, heard_at });
            });
        }
        // After the audio ends, finish what is in flight and answer the last words, then stop.
        if let Some(t) = hear_done_at {
            if (!agent_busy && inflight == 0 && fresh_words == 0) || t.elapsed() > Duration::from_secs(25) {
                break;
            }
        }
    }
    log.log(json!({"ev": "summary", "summary": &summary, "p50": summary.pct(0.5), "p95": summary.pct(0.95), "cache": engine.cache.stats()}));
    Ok(summary)
}
