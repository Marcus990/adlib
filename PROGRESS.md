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

## 2026-09-19 — session 1, continued (autonomous, /goal set)

### Git
- `a` (github.com/Marcus990/a) is an empty repo — nothing to install. Using plain git; commits per unit.

### Decisions
- **OpenRouter for both hosted calls** (user: "openrouter for jev"). One `OPENROUTER_API_KEY`.
  - Jev: `POST https://openrouter.ai/api/alpha/decisions`, `{model:"typesafe/jev-latest", state, questions}`;
    response `answers.action.{choice, probabilities, confidence}` (same as TypeSafe native).
  - Query model: `POST /api/v1/chat/completions`, `google/gemini-2.5-flash-lite`, JSON object output,
    `provider.sort=latency`, 700 ms timeout → local noun-phrase fallback.
- Every Chunk event (each `curr` update and each final) gets a fresh id (= decision seq), because each
  fans out to its own Jev + query calls and the stage joins by id. Contract unchanged.
- Stage `SearchOutcome` (chunk_id + Option<Match>) is internal to the join, not a contract change.
- img URLs are `img://localhost/<id>` (macOS WKWebView custom-scheme form).
- Offline mode (no key): decider = transparent vocabulary heuristic, query = noun-phrase fallback.

### Measured
- MobileCLIP dev-library calibration (43 built-in macOS account pictures, `scripts/make_dev_library.sh`):
  template "a photo of {}" → 12/12 correct top-1 (vs 7/12 without). Correct 0.479–0.620, best
  non-match 0.470 → τ = 0.475 (margin only 0.009: RECALIBRATE on the demo library).
- Indexing 43 images: 74 s, CPU, batch 1, peak < 2.6 GB.
- First headless e2e replay (fixtures/audio/dev-talk.wav, 48 s, `say`-synthesized):
  6 renders, all correct; chunk-end→render p50 274 ms (offline, no network); ASR p50 210 ms;
  search 68 ms (3 phrases). Command: `./target/release/ls-replay fixtures/audio/dev-talk.wav`.

### How to run
- Index: `./target/release/ls-index models/mobileclip-s2 <library-dir> <library-dir>/index.json`
- Headless: `./target/release/ls-replay <talk.wav>` or `ls-replay --mic [AirPods] --seconds 180`
- App: `LS_SOURCE=mic:AirPods LS_FULLSCREEN=1 ./target/release/live-slides` (or `LS_SOURCE=wav:<path>`)
- Logs: `logs/run-<epoch>.jsonl` (chunk/decide/search/join/render/frontend_ack with timings)

### App + rehearsal results (offline mode: no API key)
- Tauri app verified end to end with `LS_SOURCE=wav:...`: frontend steps logged via the `fe` command
  (`loaded`, `received`, `decoded`, `painted`). emit→received 2–6 ms; img:// decode 4–8 ms (LRU cache).
  With the display asleep, rAF stalls → `painted` falls back to a 100 ms timer (expected ~16 ms when awake).
- `screencapture` fails ("could not create image from display") while the user's display sleeps, so
  visual checks are via the frontend step log. A human should eyeball the stage once.
- Whisper temperature fallback caused 2.2 s ASR spikes on the first short `curr` of an utterance →
  disabled (temperature_inc 0). ASR max now 412 ms. VAD moved to CPU (1.9 s Metal-contention spike gone).
- Tried batching the 1–3 query phrases into one MobileCLIP forward pass: **rejected** — Candle's
  OpenCLIP text encoder has no padding mask, embeddings drift (cos 0.963). Per-phrase embedding kept
  (~21 ms each); guarded by `crates/search/tests/model.rs` (`cargo test --release -p ls-search -- --ignored`).
- 3-min rehearsal fixture: `fixtures/audio/rehearsal-3min.{txt,wav,expected.tsv}` (125 s, 13 subjects incl. a
  red→white rose refinement). Score with `python3 scripts/eval_run.py <log> fixtures/audio/rehearsal-3min.expected.tsv`.
  - Run A (offline): 15/15 renders correct, 13/13 subjects, min gap 4.01 s, keyword-in-transcript→render p50 70 ms
    (+ ≤ 0.75 s ASR tick ⇒ ~0.1–0.9 s from the spoken word). The only slow render (2.4 s) was a hold.
  - Run B (invalid OpenRouter key → real 401s → fallbacks): 15/15 correct, decide p50 30 ms, query p50 17 ms.
- Dev-library captions fixed ("whiterose" → "white rose", "8ball" → "eight ball"); re-indexed in 65 s.

### Hosted-model path tested without a key (mock OpenRouter)
- `scripts/mock_openrouter.py <captions.tsv> 8787` serves Jev-shaped `/api/alpha/decisions` and chat-shaped
  `/api/v1/chat/completions` with vendor-like latency (Jev 70–500 ms skewed low, chat 300–550 ms) and
  fault injection (FAIL_RATE, SLOW_RATE). Point the app at it with `OPENROUTER_BASE_URL=http://127.0.0.1:8787 OPENROUTER_API_KEY=mock`.
- Clean latency run: 15/15 correct, all 151 decisions via the Jev path, decide p50 179 ms, query p50 427 ms,
  keyword-in-transcript→render p50 428 ms (+ ≤ 0.75 s ASR tick ⇒ ~0.4–1.2 s from the word). Query branch dominates.
- Fault run (20% HTTP 500, 10% 1.5 s stalls): found Jev 900 ms + LLM-fallback 900 ms > 1 s join window →
  54 decisions dropped. Fix: total decide budget 1.1 s (Jev 700 ms; LLM fallback only with ≥ 250 ms left,
  else local heuristic). After: 2 dropped, 15/15 correct, p50 469 ms.
