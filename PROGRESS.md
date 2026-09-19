# Progress

## 2026-09-19 — session 1

### Environment (verified)
- Machine: Apple M2, 8 GB RAM, macOS 26.6.2, ~18 GB free disk. (Doc assumed an M1 Air; no plan impact.)
- Toolchain present: cargo/rustc, node 22 + npm, python3, cmake, brew, gh. No pnpm.
- No Live Slides repo existed; created this repo at `~/Programming_Projects/Hackathons/live-slides`.
- Design doc exists only as a Claude Doc (link in PLAN.md).
- `a` is not installed (`command not found`). Using plain git until it is.
- No JEV / Gemini keys in the environment.
- AirPods paired but not connected; no SD card mounted.
- crates.io cache is empty for our deps → every build needs downloads.

### Done
- Workspace + `crates/contracts` with the §8 types (derives added; shapes unchanged). Not yet compiled.

### Blockers
- Download permission (crates, npm, models, `a`), API keys (Jev, Gemini). Asked the user.

### Next
- Once unblocked: `cargo test -p ls-contracts`, commit, then run spikes S3/S4 (local), S1/S2 (keys).

### Spike S3 — whisper-rs + VAD (done)
- `./target/release/spike-whisper models/ggml-<m>.bin models/ggml-silero-v5.1.2.bin fixtures/audio/sample.wav`
  (fixture made with `say -v Samantha -o ... --data-format=LEI16@16000`)
- base.en: 224 / 303 / 353 / 437 / 457 ms for 1/2/4/6/8 s prefixes (greedy, 4 threads, Metal). Good text.
- small.en: 807–1284 ms → too slow for ~1 s `curr` re-transcription. **Chosen: base.en.**
- Silero VAD (whisper.cpp built-in, `WhisperVadContext`): 48 ms for 12 s audio; 3 correct segments
  (timestamps are centiseconds). First Metal model load can take ~13 s → warm up at startup.

### Spike S4 — Candle MobileCLIP (partial)
- Candle 0.11 supports MobileCLIP **v1** S1/S2 only (not v2). Using v1 S2 (`apple/MobileCLIP-S2-OpenCLIP`).
- Needed `rustup update stable` (1.88 → 1.98) for candle-core.
- CPU, 8 images: image encoder 50 s total (~6 s/image) — too slow; text encoder 41 ms (warm) — fine.
  Retrieval sanity: "a bird of prey" → eagle, "a red flower" → dahlia (top-1 correct on 8 images).
- **INCIDENT: crashed the user's laptop.** Metal F32 image encoding (batch 8) exhausted the 8 GB
  machine (~4.7 GB swap). Worse, a harness timeout backgrounded the first run instead of killing it, so
  a second copy ran concurrently.

### Resource guardrails (mandatory from now on)
- Never run two heavy jobs at once; check `pgrep -fl spike|cargo|rustc` before starting one.
- Wrap every heavy run in `timeout <N>` (the harness moves timed-out commands to the background; it does
  NOT kill them) and in `scripts/guard.sh` (kills the process if RSS > 2 GB).
- Builds: `CARGO_BUILD_JOBS=2`.
- Image embedding: batch size 1, run offline once, never at talk time. Try MobileCLIP S1 and F16 on Metal
  before anything larger; fall back to CPU with batch 1.
