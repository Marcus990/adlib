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

### Final unattended verification (clean build)
- `cargo clean` + `CARGO_BUILD_JOBS=2 cargo build --release --workspace`: OK in 8 m 50 s, no warnings.
- Tests: 32 unit + 1 model test (`--include-ignored`) all pass.
- App (`live-slides`, `LS_SOURCE=wav:` rehearsal) against mock OpenRouter: 15/15 correct, 151/151 decisions via
  Jev path, 0 fallbacks, Jev p50 155 ms, query p50 435 ms, keyword-in-transcript→render p50 512 ms, 15/15 painted,
  end grid shown.
- Harness bug found+fixed: Python `HTTPServer.server_bind` does reverse-DNS `getfqdn()` before listening; with the
  network asleep it hung (socket CLOSED, every request timed out) → mock now skips it. The ~20% "fallbacks" seen in
  two intermediate runs were this mock bug, not the client.
- Live mic still blocked: CoreAudio device lookup blocks for this unattended process (all devices; 5 s timeout
  triggers). Needs the user at the machine (mic permission). `ls-hear --probe AirPods 3` / `scripts/morning_check.sh`.

### Live capture verified (user present, 2026-09-19 morning)
- Mic access now works (`ls-hear --probe "MacBook Air Microphone" 5` → 4.96 s of audio). The overnight
  device-lookup hang was the idle/unattended session.
- System output was muted (volume 0), so an acoustic speakers→mic self-test heard nothing (did not change
  the user's volume). Instead: `say -a "BlackHole 2ch"` into the BlackHole virtual input, app capturing
  live with `LS_SOURCE=mic:BlackHole` (real-time CoreAudio capture path, 48 kHz → 16 kHz).
- Live app run, 2-min rehearsal talk: **15/15 correct, 13/13 subjects, 15/15 painted, min gap 4.0 s,
  keyword-in-transcript→render p50 71 ms** (local fallbacks; no API key yet).
- Remaining for the goal's exact configuration: AirPods as the input device, OPENROUTER_API_KEY for real Jev +
  query model, then the demo library/talk.

### Display intent + canvas mode (2026-09-19 afternoon)
- User idea: Jev asks "is the presenter signalling the audience should SEE something?" (intent noul + kind
  choice), not "was something picturable mentioned?". Offline heuristic needs a presentational cue
  ("here's", "take a look", "picture this", …) near a library subject. `DECIDE_MODE=topic` keeps the old behaviour.
- Canvas mode (default; `LS_MODE=single` for one full-screen image): evolving board of ≤ 4 tiles + ≤ 3
  annotations (crates/canvas), layouts auto/hero/compare/grid; canvas agent (crates/agent) over OpenRouter
  tool calling (CANVAS_MODEL, default anthropic/claude-haiku-4.5) with offline cue rules; agent triggered by
  board changes and by layout cues in *new* words only (38 → 14 calls on the fixture); version-checked ops.
- Partial-transcript confirmation (stage `confirm_partials`, `CONFIRM=0` to disable): "here's what a bald…"
  was transcribed mid-word as "what a ball is" → an 8-ball flashed up. New images from in-progress phrases
  now need two agreeing updates (finals show immediately).
- ASR tick 750 → 500 ms (ASR_TICK_MS): keyword-in-transcript→render p50 877 → 620 ms with confirmation on.
- Fixture `fixtures/audio/canvas-talk.{txt,wav,expected.tsv}` (cues, layout cues, and non-intent mentions that
  must NOT show; eval reports false_positives): 6/6 correct, 0 false positives, compare/highlight/clear/update all fire.
- The old rehearsal-3min talk has no display cues → under intent mode it (correctly) shows almost nothing;
  use canvas-talk for intent/canvas regression, rehearsal-3min with DECIDE_MODE=topic.

## 2026-09-19 — first live test by the user (MacBook mic, hosted models)
- Worked: "here's our red rose" → red rose (~1.7 s), "make it a white rose" → in-place update; plain mentions ignored.
- Fixed: (1) the query model kept repeating the on-screen caption as a 2nd phrase and search picked it
  (["panther","white rose"] → white rose) → `pick_avoiding`: the on-screen image can't win via a secondary
  phrase + prompt says don't repeat it. (2) Real Jev scores for genuine requests were 0.45–0.66 (intent ×
  kind) vs `p_render` 0.6 → p_render 0.45, τ_intent 0.6 → 0.5 (plain mentions sit at P(intent) ≤ 0.07).
  (3) Whisper initial prompt = library subjects ("red roads" → "red rose"). (TODO B2)
- Asked for panda / panther / heron — not in the 43-image dev library (library gap, expected).
- Re-verified on canvas-talk with real models: 6/6, 0 false positives, keyword→render p50 1150 ms.

## 2026-09-19 — second live test (AirPods, noisy room) → ASR fixes
- Live: 168 chunks of mostly abstract talk, 1 image ("prize track" → medal, no cue needed); supplement gate
  stayed quiet on garble. Problems: Whisper base.en garbled noisy speech + repetition loops; the vocab
  prompt leaked library words ("white rose, blue rose, blue rose…") → removed (WHISPER_VOCAB=1 opt-in).
- Fixes: repeat-collapse in clean(), suppress non-speech tokens, token cap per window; mic sessions are
  recorded to logs/run-*.wav (replay with ls-replay); query timeout 900 ms; `_exit` on quit (no ggml abort).
- ASR bake-off on fixtures/audio/canvas-talk-noisy.wav (TTS + competing talker −9 dB + hiss + low-pass):

  | model | audio ctx | WER | keywords | asr p50 / p90 |
  |---|---|---|---|---|
  | base.en | full 30 s | 45.7% | 4/10 | 293 / 408 ms |
  | base.en | floor 768 | 30.4% | 7/10 | 170 / 248 ms |
  | small.en | full | 17.4% | 10/10 | 963 / 1241 ms |
  | **small.en** | **floor 768 (15 s)** | **16.3%** | **10/10** | **423 / 641 ms** |
  | small.en | exact length / floor 512 | 28.3% | 9/10 | ~320 / 600 ms |

  → default small.en + audio-ctx floor 768 + ASR tick 600 ms. Full pipeline, real models: noisy talk 6/6,
  0 false positives, keyword→render p50 1384 ms; clean talk 6/6 (+ golf, now expected in supplement mode).

## 2026-09-19 — live diagrams + charts
- See CANVAS.md "Live diagrams and charts". New fixture: fixtures/audio/graphics-talk.wav (+ .txt).
- Also: τ 0.477 → 0.52 (junk phrases matched at 0.43–0.49, real matches 0.54–0.61); partial-confirmation
  candidates expire after 2.5 s (two weak matches 11 s apart had "confirmed" an Earth photo).
- Probe harness for agent prompts: `cargo test -p ls-agent dump_tools -- --ignored --nocapture` + a JSON of
  board/speech cases → call OpenRouter directly (faster than a 90 s replay per prompt change).

## 2026-09-19 — Jev sees the whole board
- `Displayed.on_screen` (one line per tile: photos, charts with values, diagrams with steps) is passed to Jev
  next to the focused photo; the question says "not already on screen, as a photo or covered by a chart or
  diagram", with refinements/variants ("make that the white one", "X and a Y") counting as new.
- A/B on real Jev (2 test talks + 2 AirPods recordings): "show" decisions −20–35%; "side-by-side of a rose and
  a white rose" now keeps both (was replaced as a refinement); canvas-talk 6/6, 0 false positives.
- Prompt probe: `cargo test -p ls-decide dump_jev_cases -- --ignored --nocapture` → POST each to
  /api/alpha/decisions (6 cases: refine, side-by-side, chart covers it, already shown, new subject, filler).

## 2026-09-19 — latency investigation (before changing anything; checkpoint tag `pre-latency`)
- Photo path ≈ 1.6–1.8 s word→screen: transcript lag 0.2–0.55 s (p50) + join ≈ 0.5 s (phrase model ≈ 0.45 s is
  the long pole; Jev ≈ 0.2 s) + partial confirmation ≈ 0.6 s (every photo today) + paint ≈ 0.1 s; back-to-back
  subjects add up to 3.3 s of hold.
- OpenRouter floor: a 1-token reply = 425–494 ms, so no phrase-model swap helps much (bake-off, 8 real moments ×2:
  gemini-2.5-flash-lite p50 518/p90 1077 16/16; 3.1-flash-lite 530/682 16/16; gpt-4.1-nano 679 16/16;
  gpt-oss-20b 521 8/16; llama-3.3-70b 1065 11/16; mistral-small-3.2 789 16/16).
- Canvas agent bake-off (6 real moments ×2): haiku-4.5 1612/2216 8/12 (pie → update_chart on the users
  chart); gemini-2.5-flash 802/1525 12/12; qwen3-235b-2507 862/1403 12/12; gemini-3.1-flash-lite 637 10/12;
  gpt-oss-120b 467 10/12 (never clears); gpt-4.1-mini 1222 7/12.
- Confirmation rule today: 42 candidates later confirmed (Jev p 0.45–0.93; 34 had p ≥ 0.6), 11 blocked for
  good — every wrong one had p ≤ 0.55 (eagle for misheard "Hangouts", white rose for "My grandfather",
  yin-yang for "this O"); one blocked was right (owl for "Auls", p 0.61–0.67).
- 34/48 photos came from a chunk that literally names the library subject.

## 2026-09-19 — latency changes (after checkpoint `pre-latency`), verified against a fresh baseline
- Photo path: (1) confident partials skip confirmation (Jev p ≥ 0.6); (2) named-subject shortcut: a chunk
  that names exactly one library subject unambiguously is searched at once, skipping the phrase model
  (`ls_query::named_subject`; "a rose and a white rose" / "owls and penguins" still go to the model);
  (3) board mode hold 4 s → 1.5 s (new photos add tiles, nothing flickers).
- Canvas guards found while testing Gemini 2.5 Flash as the agent (kept for Haiku): chart-kind jumps / unrelated
  data sets become a NEW chart (Haiku and Gemini both turned the users bar chart into the pie); a same-layout
  redraw replaces the diagram in focus; `remove` only on a removal cue ("get rid of", "take away"…).
- Agent model: stayed on Claude Haiku 4.5 (user decision). Gemini 2.5 Flash was ~2× faster but tidied the board
  (removals) and skipped a clear in real replays; `CANVAS_MODEL=google/gemini-2.5-flash` to retry (thinking off).
- Same 4 inputs, baseline → after (real models): canvas-talk 6/6, 0 FP both; keyword→photo p50 1232 → 402 ms;
  14:40 AirPods word→photo 1685 → 337 ms; 15:05 AirPods 3310 → 1508 ms (and keeps the sunflower the baseline
  missed; both roses side by side); graphics-talk final board identical (users bars, 60/30/10 pie, cycle);
  graphics latency unchanged (Haiku, ~1.5–2.5 s after the sentence).
