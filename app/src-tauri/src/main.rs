//! Track E (shell): Tauri windows + `img://` protocol + render/status events. Rust owns every
//! decision; the web views only present (design doc §4).
//!
//! Env: LS_SOURCE = "mic" (default) | "mic:<device substring>" | "wav:<path>"; LS_FULLSCREEN=1;
//! LS_DISPLAY = monitor index (0 = primary) or name substring for the stage window (e.g. a projector);
//! LS_ROOT = repo root (defaults to the workspace this binary was built from).

use ls_pipeline::{run, AudioSource, Config, Engine, Logger, RenderSink};
use ls_search::ImageCache;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Latest app state, so a window that loads late (or reloads) can ask for it instead of
/// depending on having been listening when an event fired.
#[derive(Clone, Default)]
struct AppState(Arc<Mutex<Value>>);

impl AppState {
    fn merge(&self, patch: Value) -> Value {
        let mut s = self.0.lock().unwrap();
        if let (Some(obj), Some(p)) = (s.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                obj.insert(k.clone(), v.clone());
            }
        }
        s.clone()
    }
}

struct TauriSink {
    app: tauri::AppHandle,
    state: AppState,
}

impl RenderSink for TauriSink {
    fn render(&self, ev: &ls_contracts::RenderEvent) {
        let _ = self.app.emit("render", ev);
    }
    fn scene(&self, s: &ls_canvas::Scene) {
        let _ = self.app.emit("scene", s);
    }
    fn status(&self, v: &Value) {
        match v["type"].as_str() {
            Some("mic") => {
                let s = self.state.merge(json!({"source": format!("mic: {}", v["device"].as_str().unwrap_or("?"))}));
                let _ = self.app.emit("state", s);
            }
            Some("mic_ok") => {
                let s = self.state.merge(json!({"phase": "listening", "error": null}));
                let _ = self.app.emit("state", s);
            }
            Some("error") => {
                let s = self.state.merge(json!({"phase": "error", "error": v["error"]}));
                let _ = self.app.emit("state", s);
            }
            _ => {}
        }
        let _ = self.app.emit_to("debug", "status", v);
    }
}

/// Frontend progress for each render event: received → decoded → painted (or an error).
#[tauri::command]
fn fe(log: tauri::State<'_, Logger>, step: String, chunk_id: Option<u64>, ms: Option<f64>, detail: Option<String>) {
    log.log(json!({"ev": "frontend", "step": step, "chunk_id": chunk_id, "ms": ms.map(|m| m.round()), "detail": detail}));
}

#[tauri::command]
fn app_state(state: tauri::State<'_, AppState>) -> Value {
    state.0.lock().unwrap().clone()
}

fn root() -> PathBuf {
    std::env::var("LS_ROOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn source() -> (AudioSource, String) {
    let s = std::env::var("LS_SOURCE").unwrap_or_else(|_| "mic".into());
    if let Some(p) = s.strip_prefix("wav:") {
        let name = std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        (AudioSource::Wav { path: p.into(), realtime: true }, format!("replay: {name}"))
    } else {
        let dev = s.strip_prefix("mic:").map(String::from);
        let label = format!("mic: {}", dev.clone().unwrap_or_else(|| "default input".into()));
        (AudioSource::Mic { device: dev }, label)
    }
}

/// Move the stage window onto the chosen monitor (index or name substring) before going full screen.
fn place_on_display(w: &tauri::WebviewWindow, want: &str) {
    let Ok(monitors) = w.available_monitors() else { return };
    let pick = want
        .parse::<usize>()
        .ok()
        .and_then(|i| monitors.get(i))
        .or_else(|| monitors.iter().find(|m| m.name().map(|n| n.to_lowercase().contains(&want.to_lowercase())).unwrap_or(false)));
    match pick {
        Some(m) => {
            let _ = w.set_position(*m.position());
        }
        None => eprintln!(
            "LS_DISPLAY={want:?} matched no monitor; available: {:?}",
            monitors.iter().map(|m| m.name().cloned().unwrap_or_default()).collect::<Vec<_>>()
        ),
    }
}

fn main() -> anyhow::Result<()> {
    let root = root().canonicalize()?;
    let cfg = Config::from_env(&root);
    let remote = cfg.api_key.is_some() || cfg.openai_key.is_some();
    let mode = "canvas";
    let log = Logger::create(&cfg.log_path)?;
    eprintln!("log: {}  remote: {}", cfg.log_path.display(), remote);
    let (src, src_label) = source();
    // LS_THEME: "sketch" (default: paper, hand-drawn strokes, taped photos) or "slate" (dark cards).
    let theme = std::env::var("LS_THEME").unwrap_or_else(|_| "sketch".into());
    let state = AppState(Arc::new(Mutex::new(json!({"phase": "loading", "source": src_label, "remote": remote, "mode": mode, "theme": theme}))));
    let engine = Arc::new(tauri::async_runtime::block_on(Engine::load(cfg))?);
    let cache = engine.cache.clone();

    tauri::Builder::default()
        .register_uri_scheme_protocol("img", move |_ctx, req| {
            let id = req.uri().path().trim_start_matches('/').to_string();
            match cache.get(&id) {
                Some(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", ImageCache::mime_of(&id))
                    .header("Access-Control-Allow-Origin", "*")
                    .body(bytes.to_vec())
                    .unwrap(),
                None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .manage(log.clone())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![fe, app_state])
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                if let Ok(d) = std::env::var("LS_DISPLAY") {
                    place_on_display(&w, &d);
                }
                if std::env::var("LS_FULLSCREEN").is_ok() {
                    let _ = w.set_fullscreen(true);
                }
            }
            let handle = app.handle().clone();
            let (engine, log) = (engine.clone(), log.clone());
            tauri::async_runtime::spawn(async move {
                engine.warm_up(&log).await;
                let sink = Arc::new(TauriSink { app: handle.clone(), state: state.clone() });
                match run(engine, src, sink, log.clone(), Arc::new(AtomicBool::new(false))).await {
                    Ok(s) => {
                        let _ = handle.emit("talk_end", ());
                        let _ = handle.emit("state", state.merge(json!({"phase": "ended"})));
                        let _ = handle.emit_to("debug", "status", json!({"type": "summary", "summary": s,
                            "p50": s.pct(0.5), "p95": s.pct(0.95)}));
                    }
                    Err(e) => {
                        log.log(json!({"ev": "pipeline_error", "error": format!("{e:#}")}));
                        let _ = handle.emit("state", state.merge(json!({"phase": "error", "error": format!("{e:#}")})));
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(|_, ev| {
            // Skip C++ static destructors on quit: ggml's Metal device destructor aborts (exit 134)
            // while the Whisper context on the hearing thread is still alive. Logs are flushed per
            // line and the session WAV header every second, so nothing is lost.
            if let tauri::RunEvent::Exit = ev {
                unsafe { libc::_exit(0) }
            }
        });
    Ok(())
}
