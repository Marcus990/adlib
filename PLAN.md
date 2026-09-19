# Live Slides — POC plan

**Source of truth for design:** the Claude Doc "Live Slides — Engineering Design Doc (48-hour proof of concept)"
https://claude.ai/code/artifact/9e06d7b5-20dd-40dd-87e6-670c4a02f815 (read with the docs tools; sections 2, 4, 7, 8 are the build spec).
Contracts from doc §8 live in `crates/contracts` (do not change casually; log changes in PROGRESS.md).

## Goal
AirPods mic → local VAD + Whisper → parallel {Jev: whether} + {query model: what} → MobileCLIP search →
Rust join/state machine → Tauri `emit("render")` → full-screen image with crossfade.
Typical change ~1–1.5 s after the relevant speech; no jarring changes in final rehearsals.

## Architecture decisions
- Cargo workspace. One crate per track plus `contracts`; the Tauri app crate wires them together.
  - `crates/contracts`: shared types (doc §8).
  - `crates/hear` (A): cpal → 16 kHz mono → Silero VAD → whisper-rs → `Chunk`; WAV replay; JSONL log.
  - `crates/search` (B): Candle MobileCLIP indexer + brute-force cosine (no ANN until >10k images); LRU; `img://`.
  - `crates/decide` (C): Jev client + state builder → `ChangeDecision`.
  - `crates/query` (D): Gemini 2.5 Flash-Lite client, 700 ms timeout, local noun-phrase fallback → `QueryResult`.
  - `crates/stage` (E, logic): join by chunk id + stage state machine → `RenderEvent`. Pure, time injected, unit-tested.
  - `app/` (E, shell): Tauri 2 + minimal TS web view (draw only).
- Rust owns all decisions; the web view only presents.
- Replay-first: every track is testable from fixtures/WAV without a mic or network.
- Brute-force search (design allows it for small libraries).
- Timing: every stage writes timestamps into the JSONL log keyed by chunk id.

## Known blockers (see PROGRESS.md for current status)
- Jev API key + access (early access) — needed for the Jev spike.
- Gemini API key — needed for the query-model spike (fallback path works without it).
- Permission to download toolchain deps + models (crates, npm, Whisper ggml, Silero VAD, MobileCLIP weights).
- `a` git tool (github.com/Marcus990/a) not installed; plain git used until installed.
- AirPods / SD card not attached; laptop mic + local folder used for development.
