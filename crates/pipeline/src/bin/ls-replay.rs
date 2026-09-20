//! Headless end-to-end run: `ls-replay <talk.wav>` (real-time paced) or `ls-replay --mic [device]
//! [--seconds N]`. Prints render events and a latency summary; full timings go to logs/run-*.jsonl.
use ls_pipeline::{run, AudioSource, Config, Engine, Logger, RenderSink};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

struct Print;
impl RenderSink for Print {
    fn render(&self, ev: &ls_contracts::RenderEvent) {
        println!("RENDER {:<7} {:<28} chunk={} t={}ms", ev.kind, ev.image_id.clone().unwrap_or_default(), ev.chunk_id, ev.ts_ms);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let root = std::env::current_dir()?;
    let cfg = Config::from_env(&root);
    let log = Logger::create(&cfg.log_path)?;
    eprintln!("log: {}  remote: {}", cfg.log_path.display(), cfg.api_key.is_some() || cfg.openai_key.is_some());
    let source = if a.get(1).map(|s| s.as_str()) == Some("--mic") {
        AudioSource::Mic { device: a.get(2).filter(|s| !s.starts_with("--")).cloned() }
    } else {
        AudioSource::Wav { path: a.get(1).expect("usage: ls-replay <wav> | --mic [device]").into(), realtime: true }
    };
    let engine = Arc::new(Engine::load(cfg).await?);
    engine.warm_up(&log).await;
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(secs) = a.iter().position(|x| x == "--seconds").and_then(|i| a.get(i + 1)).and_then(|s| s.parse::<u64>().ok()) {
        let s = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            s.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }
    let summary = run(engine, source, Arc::new(Print), log, stop).await?;
    println!("\nchunks={} renders={} shown={:?}", summary.chunks, summary.renders, summary.shown);
    println!("speech→render latency ms: {:?}  p50={:?} p95={:?}", summary.render_latencies_ms, summary.pct(0.5), summary.pct(0.95));
    println!("agent calls={} fallbacks={} ops applied={} refused={} generated={}", summary.agent_calls, summary.agent_fallbacks, summary.ops_applied, summary.ops_refused, summary.generated);
    Ok(())
}
