//! Track E (shell): Tauri window + `img://` protocol + render/status events. Rust owns every
//! decision; the web view only presents (design doc §4).
//!
//! Env: LS_SOURCE = "mic" (default) | "mic:<device substring>" | "wav:<path>"; LS_FULLSCREEN=1;
//! LS_ROOT = repo root (defaults to the workspace this binary was built from).

use ls_pipeline::{run, AudioSource, Config, Engine, Logger, RenderSink};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{Emitter, Manager};

struct TauriSink {
    app: tauri::AppHandle,
}

impl RenderSink for TauriSink {
    fn render(&self, ev: &ls_contracts::RenderEvent) {
        let _ = self.app.emit("render", ev);
    }
    fn status(&self, v: &Value) {
        let _ = self.app.emit_to("debug", "status", v);
    }
}

/// Frontend progress for each render event: received → decoded → painted (or an error).
#[tauri::command]
fn fe(log: tauri::State<'_, Logger>, step: String, chunk_id: Option<u64>, ms: Option<f64>, detail: Option<String>) {
    log.log(json!({"ev": "frontend", "step": step, "chunk_id": chunk_id, "ms": ms.map(|m| m.round()), "detail": detail}));
}

fn root() -> PathBuf {
    std::env::var("LS_ROOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn source() -> AudioSource {
    let s = std::env::var("LS_SOURCE").unwrap_or_else(|_| "mic".into());
    if let Some(p) = s.strip_prefix("wav:") {
        AudioSource::Wav { path: p.into(), realtime: true }
    } else {
        AudioSource::Mic { device: s.strip_prefix("mic:").map(String::from) }
    }
}

fn main() -> anyhow::Result<()> {
    let root = root().canonicalize()?;
    let cfg = Config::from_env(&root);
    let log = Logger::create(&cfg.log_path)?;
    eprintln!("log: {}  remote: {}", cfg.log_path.display(), cfg.api_key.is_some());
    let engine = Arc::new(tauri::async_runtime::block_on(Engine::load(cfg))?);
    let cache = engine.cache.clone();

    tauri::Builder::default()
        .register_uri_scheme_protocol("img", move |_ctx, req| {
            let id = req.uri().path().trim_start_matches('/').to_string();
            match cache.get(&id) {
                Some(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", ls_search_mime(&id))
                    .header("Access-Control-Allow-Origin", "*")
                    .body(bytes.to_vec())
                    .unwrap(),
                None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .manage(log.clone())
        .invoke_handler(tauri::generate_handler![fe])
        .setup(move |app| {
            if std::env::var("LS_FULLSCREEN").is_ok() {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_fullscreen(true);
                }
            }
            let handle = app.handle().clone();
            let (engine, log) = (engine.clone(), log.clone());
            tauri::async_runtime::spawn(async move {
                engine.warm_up(&log).await;
                let sink = Arc::new(TauriSink { app: handle.clone() });
                let _ = handle.emit_to("debug", "status", json!({"type": "ready"}));
                match run(engine, source(), sink, log.clone(), Arc::new(AtomicBool::new(false))).await {
                    Ok(s) => {
                        let _ = handle.emit_to("debug", "status", json!({"type": "summary", "summary": s,
                            "p50": s.pct(0.5), "p95": s.pct(0.95)}));
                    }
                    Err(e) => log.log(json!({"ev": "pipeline_error", "error": format!("{e:#}")})),
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

fn ls_search_mime(id: &str) -> &'static str {
    // Index ids have no extension; the library is JPEG unless the id says otherwise.
    if id.ends_with(".png") { "image/png" } else if id.ends_with(".webp") { "image/webp" } else { "image/jpeg" }
}
