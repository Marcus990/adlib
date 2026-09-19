# Live Slides — 48-hour proof of concept

A presenter talks; the screen shows one full-bleed image from a local library that matches what is
being said, changing on its own. Design doc: see PLAN.md (link). State of the build: PROGRESS.md / TODO.md.

```
mic → VAD + Whisper (local) → ┬→ Jev "change?" (OpenRouter)            ┐
                              └→ query model "what?" (OpenRouter) → MobileCLIP search (local)
                                                                        → join + stage rules (Rust) → Tauri render
```

## One-time setup (8 GB Mac: run heavy steps one at a time)

1. Models (already downloaded into `models/`): `ggml-base.en.bin`, `ggml-silero-v5.1.2.bin`,
   `mobileclip-s2/{open_clip_model.safetensors,tokenizer.json}`.
2. `.env` — copy `.env.example`, set `OPENROUTER_API_KEY`. Without it the app still runs, using the local
   fallbacks (vocabulary heuristic for "change?", noun phrases for "what?").
3. Build: `CARGO_BUILD_JOBS=2 cargo build --release`
4. Image library: a folder of jpg/png/webp + optional `captions.tsv` (`id<TAB>caption`, id = file stem).
   Index it (one image at a time, ~1.5 s each):
   `./scripts/guard.sh 2600 ./target/release/ls-index models/mobileclip-s2 <lib> <lib>/index.json`
   Dev library from macOS built-ins: `./scripts/make_dev_library.sh` → `dev-library/`.
5. Check search quality: `./target/release/ls-search models/mobileclip-s2 <lib>/index.json "golden eagle" "a red rose"`

## Run

- **Pick the mic by name.** On this Mac the default input is "BlackHole 2ch" (a virtual loopback), so
  `LS_SOURCE=mic` alone would hear silence. Use `mic:AirPods` (or `mic:MacBook Air Microphone`).
  The first live run will trigger macOS's microphone permission prompt for the terminal/app; if the
  device lookup blocks for 5 s the app reports "audio device lookup timed out" in the debug window.
- Demo (AirPods): `INDEX=<lib>/index.json LS_SOURCE=mic:AirPods LS_FULLSCREEN=1 ./target/release/live-slides`
  - Two windows: the full-screen stage and a debug window (live transcript, decisions, phrases, timings).
  - Stage keys: `f` full screen, `Esc` leave full screen, `b` blank the screen (safety valve), `g` grid.
  - `LS_DISPLAY=1` (monitor index) or `LS_DISPLAY=<name part>` puts the stage on that display (projector).
  - `f` toggles full screen on the stage window; `g` toggles the grid of every image shown so far
    (the "deck that built itself"; it also appears automatically when a replay ends).
- Rehearsal replay of a recording: `LS_SOURCE=wav:talk.wav ./target/release/live-slides`
- Headless (no UI) replay with a summary: `./target/release/ls-replay talk.wav`
- Every run writes `logs/run-<epoch>.jsonl`: chunk (asr/vad ms, lag), decide (source, action, p, ms),
  search (phrases, fallback, query/search ms, best + score), join outcome, render (speech→render ms),
  frontend_ack (decode + receive→paint ms).

## Tuning knobs
- `TAU` (image score threshold, default 0.477 — recalibrate per library with `ls-calibrate` (labels TSV: phrase<TAB>image_id or -)).
- Stage holds/probabilities: `crates/stage` `StageConfig` (4 s render hold, 1.5 s update, p ≥ 0.6/0.7).
- Chunking: `crates/hear` `ChunkerConfig` (0.75 s tick, 0.6 s pause, 8 s max).
