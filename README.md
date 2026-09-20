# Live Slides — 48-hour proof of concept

A presenter talks; the screen shows one full-bleed image from a local library that matches what is
being said, changing on its own. Design doc: see PLAN.md (link). State of the build: PROGRESS.md / TODO.md.

```
mic → VAD + Whisper (local) → transcript → Luna (OpenAI API, or OpenRouter) → board ops / show_photo
                                  Luna sees the whole transcript, the board, and what it changed recently
                       show_photo → CLIP search of the photo library (local) → or draw it → Tauri render
```

## One-time setup (8 GB Mac: run heavy steps one at a time)

1. Models (already downloaded into `models/`): `ggml-base.en.bin`, `ggml-silero-v5.1.2.bin`,
   `mobileclip-s2/{open_clip_model.safetensors,tokenizer.json}`.
2. `.env` — copy `.env.example`, set `OPENAI_API_KEY` (Luna on OpenAI's own API, fastest) or `OPENROUTER_API_KEY`.
   Without either the app still runs on the offline rules (layout cues, and a presenter cue + library subject
   for photos).
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
- End-to-end check: `./target/release/ls-replay fixtures/audio/luna-edit-talk.wav`, then
  `python3 scripts/e2e_check.py logs/run-….jsonl` (nine board milestones, in order).
- Agent probes (no audio): `cargo run -p ls-agent --bin ls-agent-probe -- --runs 3` (see probes/luna/README.md).
- Headless (no UI) replay with a summary: `./target/release/ls-replay talk.wav`
- Every run writes `logs/run-<epoch>.jsonl`: chunk (asr/vad ms, lag), agent_call / agent (Luna's ops, transport,
  first WebSocket event and total ms, what applied, what was refused and why, `no_action` reasons), photo_search (subject, best + score), generated, render
  (speech→render ms), scene (the board after each change), frontend_ack (decode + receive→paint ms).

## Tuning knobs
- `OPENAI_API_KEY` / `OPENROUTER_API_KEY` — Luna's backend: OpenAI's own API when its key is set, else OpenRouter
  (`CANVAS_PROVIDER=openrouter` forces OpenRouter). `CANVAS_MODEL` (default `gpt-5.6-luna`; on OpenRouter it is
  `openai/gpt-5.6-luna`, the prefix is added or dropped for you). OpenAI calls use the standard service tier by
  default and log the tier returned; `CANVAS_SERVICE_TIER=fast` opts into Fast mode. `CANVAS_TRANSPORT=websocket` enables OpenAI's
  persistent Responses connection: startup prepares the prompt and tools without generating, later turns send
  incremental speech and canvas outcomes, and any socket failure retries over HTTP. HTTP remains the default until
  this path matches the established full probe baseline.
  `CANVAS_TIMEOUT_MS` (6000): one retry, shorter, on a
  timeout / 429 / 5xx, then the offline rules. `AGENT_RPM` — calls per minute: 500 on OpenAI (the measured maximum;
  the account also allows 500k tokens a minute, and HTTP calls carry the whole transcript), 18 on OpenRouter (a new
  account is capped at 20/min for Luna).
- `LS_ASSETS` (asset card root, e.g. `/Volumes/NO NAME/assets`) — Marcus's 15k-photo library (OpenAI CLIP ViT-B/32
  embeddings, see ASSETS_HANDOFF.md). Unset = the local MobileCLIP index (`INDEX`, `CLIP_DIR`).
- `CLIP_TEXT_DIR` (default `models/clip-vit-b32`) — `tokenizer.json` + `pytorch_model.bin` from
  openai/clip-vit-base-patch32; the text tower is extracted once into `clip-text-vit-b32.safetensors`.
- `BASETEN_API_KEY` (+ optional `BASETEN_URL`, `GEN_SIZE`, default 768) — draws a picture when the library has
  nothing. ~2 s warm; the deployment is woken at launch because a cold start takes ~146 s.
- `TAU` — lowest photo score accepted (0.22 on the asset card, 0.52 with MobileCLIP; recalibrate per library with
  `ls-calibrate`). `LABEL_MIN` (0.92) / `UNLABELLED_MIN` (off) — how strictly a card photo must match.
- `LS_THEME` (`sketch` = paper + hand-drawn graphics, default; `slate` = dark cards).
- Chunking: `crates/hear` `ChunkerConfig` (0.6 s tick, 0.6 s pause, 8 s max).
