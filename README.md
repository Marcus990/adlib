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

**Double-clickable app:** `./scripts/make_app.sh` → `build/Live Slides.app` (icon, mic-permission text).
Put settings in `.env` (it's loaded at startup), e.g. `LS_SOURCE=mic:AirPods`, `LS_FULLSCREEN=1`, `LS_DISPLAY=1`.
First launch: macOS asks for microphone access for "Live Slides" — click Allow (the app keeps retrying and
starts listening as soon as it's granted; the debug window says "waiting for microphone"). The very first
launch also compiles Metal shaders (~13 s "loading models").
With no mic named, the app prefers AirPods, then the MacBook mic, and never a virtual device.

**Launcher:** `./demo.sh [window] airpods|builtin|replay [display|wav]` (full screen by default).

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
- `LS_ASSETS` (asset card root, e.g. `/Volumes/NO NAME/assets`) — switches photo search to Marcus's 15k-photo
  library (OpenAI CLIP ViT-B/32 embeddings, see ASSETS_HANDOFF.md). Unset = the local MobileCLIP index.
- `CLIP_TEXT_DIR` (default `models/clip-vit-b32`) — `tokenizer.json` + `pytorch_model.bin` from
  openai/clip-vit-base-patch32; the text tower is extracted once into `clip-text-vit-b32.safetensors`.
- `BASETEN_API_KEY` (+ optional `BASETEN_URL`, `GEN_SIZE`, default 768) — draws a picture when the library has
  nothing. ~2 s warm; the deployment is woken at launch because a cold start takes ~146 s.
- `LABEL_MIN` (0.92) / `UNLABELLED_MIN` (off) — how strictly a card photo must match the query.
- `LS_THEME` (`sketch` = paper + hand-drawn graphics, default; `slate` = dark cards).
- `TAU` (image score threshold, default 0.52 — recalibrate per library with `ls-calibrate` (labels TSV: phrase<TAB>image_id or -)).
- Stage holds/probabilities: `crates/stage` `StageConfig` (4 s render hold, 1.5 s update, p ≥ 0.6/0.7).
- Chunking: `crates/hear` `ChunkerConfig` (0.75 s tick, 0.6 s pause, 8 s max).
