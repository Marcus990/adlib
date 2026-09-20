//! Track E (shell): Tauri windows + `img://` protocol + render/status events. Rust owns every
//! decision; the web views only present (design doc §4).
//!
//! Env: LS_SOURCE = "mic" (default) | "mic:<device substring>" | "wav:<path>"; LS_FULLSCREEN=1;
//! LS_DISPLAY = monitor index (0 = primary) or name substring for the stage window (e.g. a projector);
//! LS_ROOT = repo root (defaults to the workspace this binary was built from).

use ls_pipeline::{run, AudioSource, Config, Engine, Logger, RenderSink};
use ls_search::ImageCache;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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

#[derive(Clone)]
struct SessionControl {
    root: Arc<PathBuf>,
    running: Arc<AtomicBool>,
    stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    cache: Arc<Mutex<Option<ls_search::ImageCache>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorSettings {
    microphone: String,
    display: String,
    fullscreen: bool,
    theme: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplayChoice {
    id: String,
    name: String,
    primary: bool,
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

#[tauri::command]
async fn list_microphones() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(ls_hear::audio::list_inputs)
        .await
        .unwrap_or_default()
}

#[tauri::command]
fn list_displays(app: tauri::AppHandle) -> Vec<DisplayChoice> {
    let Some(stage) = app.get_webview_window("main") else { return vec![] };
    let Ok(monitors) = stage.available_monitors() else { return vec![] };
    let primary_name = stage.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let name = m.name().cloned().unwrap_or_else(|| format!("Display {}", i + 1));
            DisplayChoice { id: i.to_string(), primary: primary_name.as_deref() == Some(name.as_str()), name }
        })
        .collect()
}

#[tauri::command]
fn set_blank(app: tauri::AppHandle, state: tauri::State<'_, AppState>, blank: bool) {
    let next = state.merge(json!({"blank": blank}));
    let _ = app.emit("state", next);
    let _ = app.emit_to("main", "operator_blank", blank);
}

#[tauri::command]
fn end_session(app: tauri::AppHandle, session: tauri::State<'_, SessionControl>, state: tauri::State<'_, AppState>) {
    if let Some(stop) = session.stop.lock().unwrap().as_ref() {
        stop.store(true, Ordering::Relaxed);
        let next = state.merge(json!({"phase": "stopping"}));
        let _ = app.emit("state", next);
    }
}

#[tauri::command]
fn start_session(
    settings: OperatorSettings,
    app: tauri::AppHandle,
    session: tauri::State<'_, SessionControl>,
    state: tauri::State<'_, AppState>,
    log: tauri::State<'_, Logger>,
) -> Result<(), String> {
    if session.running.swap(true, Ordering::SeqCst) {
        return Err("A presentation is already running".into());
    }
    let source = AudioSource::Mic {
        device: (!settings.microphone.trim().is_empty()).then_some(settings.microphone.clone()),
    };
    let source_label = if settings.microphone.trim().is_empty() {
        "mic: automatic".to_string()
    } else {
        format!("mic: {}", settings.microphone)
    };
    let stop = Arc::new(AtomicBool::new(false));
    *session.stop.lock().unwrap() = Some(stop.clone());

    if let Some(stage) = app.get_webview_window("main") {
        let _ = app.emit_to("main", "session_reset", ());
        if !settings.display.trim().is_empty() {
            place_on_display(&stage, &settings.display);
        }
        let _ = stage.show();
        let _ = stage.set_fullscreen(settings.fullscreen);
    }
    let next = state.merge(json!({
        "phase": "loading",
        "source": source_label,
        "theme": settings.theme,
        "blank": false,
        "error": null
    }));
    let _ = app.emit("state", next);

    let root = session.root.clone();
    let running = session.running.clone();
    let stop_slot = session.stop.clone();
    let cache_slot = session.cache.clone();
    let state = state.inner().clone();
    let log = log.inner().clone();
    tauri::async_runtime::spawn(async move {
        let cfg = Config::from_env(&root);
        let remote = cfg.api_key.is_some() || cfg.openai_key.is_some();
        let mode = "canvas";
        let next = state.merge(json!({"remote": remote, "mode": mode}));
        let _ = app.emit("state", next);
        let completed = match Engine::load(cfg).await {
            Ok(engine) => {
                let engine = Arc::new(engine);
                *cache_slot.lock().unwrap() = Some(engine.cache.clone());
                engine.warm_up(&log).await;
                let sink = Arc::new(TauriSink { app: app.clone(), state: state.clone() });
                match run(engine, source, sink, log.clone(), stop).await {
                    Ok(summary) => {
                        let _ = app.emit("talk_end", ());
                        let _ = app.emit_to("debug", "status", json!({"type": "summary", "summary": summary,
                            "p50": summary.pct(0.5), "p95": summary.pct(0.95)}));
                        true
                    }
                    Err(e) => {
                        log.log(json!({"ev": "pipeline_error", "error": format!("{e:#}")}));
                        let _ = app.emit("state", state.merge(json!({"phase": "error", "error": format!("{e:#}")})));
                        false
                    }
                }
            }
            Err(e) => {
                log.log(json!({"ev": "engine_error", "error": format!("{e:#}")}));
                let _ = app.emit("state", state.merge(json!({"phase": "error", "error": format!("{e:#}")})));
                false
            }
        };
        running.store(false, Ordering::SeqCst);
        *stop_slot.lock().unwrap() = None;
        if completed {
            if let Some(stage) = app.get_webview_window("main") {
                let _ = stage.set_fullscreen(false);
                let _ = stage.hide();
            }
            let next = state.merge(json!({
                "phase": "idle", "source": "not started", "blank": false, "error": null
            }));
            let _ = app.emit("state", next);
        }
    });
    Ok(())
}

fn root() -> PathBuf {
    std::env::var("LS_ROOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
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
    let state = AppState(Arc::new(Mutex::new(json!({
        "phase": "idle", "source": "not started", "remote": remote, "mode": mode,
        "theme": "sketch", "blank": false
    }))));
    let cache: Arc<Mutex<Option<ls_search::ImageCache>>> = Arc::new(Mutex::new(None));
    let session = SessionControl {
        root: Arc::new(root),
        running: Arc::new(AtomicBool::new(false)),
        stop: Arc::new(Mutex::new(None)),
        cache: cache.clone(),
    };

    tauri::Builder::default()
        .register_uri_scheme_protocol("img", move |_ctx, req| {
            let id = req.uri().path().trim_start_matches('/').to_string();
            let bytes = cache.lock().unwrap().as_ref().and_then(|c| c.get(&id));
            match bytes {
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
        .manage(session)
        .invoke_handler(tauri::generate_handler![
            fe, app_state, list_microphones, list_displays, start_session, end_session, set_blank
        ])
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
