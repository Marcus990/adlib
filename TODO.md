# TODO

## Setup
- [x] Repo, workspace, contracts crate; `cargo test -p ls-contracts` (3 tests)
- [x] `a` tool — github.com/Marcus990/a is an EMPTY repo (no commits) → plain git
- [x] `.env.example` (OPENROUTER_API_KEY, QUERY_MODEL, JEV_MODEL)
- [ ] User: put OPENROUTER_API_KEY in `.env` (one key covers Jev + query model)

## Phase 0 spikes
- [x] S3 whisper-rs + Silero VAD → base.en (224–457 ms per 1–8 s window)
- [x] S4 Candle MobileCLIP → v1 S2 on CPU, 1 image at a time (~1.5 s/img index; 21 ms/text query)
- [~] S1 Jev: request/response shape verified from docs (OpenRouter /api/alpha/decisions); live timing blocked on key
- [~] S2 Query model: client + fallback built; live timing/quality blocked on key

## Tracks
- [x] A Hear: chunker (curr ~0.8 s, finals on pauses, 8 s cap), whisper + VAD, WAV replay, cpal mic, ls-hear bin, 6 tests
- [x] B Search: indexer (ls-index), scoring §7.3, template "a photo of {}", τ=0.475 (dev lib), LRU cache, ls-search bin, 3 tests
- [x] C Decide: Jev via OpenRouter, LLM fallback, offline heuristic, 5 tests
- [x] D Query: OpenRouter chat JSON, 700 ms timeout, noun-phrase fallback, 5 tests
- [x] E Stage: join + state machine, 10 tests
- [x] E Show: Tauri app (img:// protocol, crossfades, blurred backdrop, debug window, frontend step timing)

## Integration
- [x] Headless e2e WAV replay (ls-replay): 6/6 correct renders on dev talk, chunk-end→render p50 274 ms (offline)
- [x] Tauri app: WAV replay → screen (3-min rehearsal 15/15, all frames received/decoded/painted)
- [ ] BLOCKED (user): live mic — CoreAudio unavailable unattended; mic permission prompt; default input is BlackHole
- [ ] BLOCKED (user): OPENROUTER_API_KEY → rerun rehearsal, record Jev/query latency + quality
- [ ] Demo library + demo talk (humans), recalibrate τ, rehearsals ×3

## Known issues / follow-ups
- [x] ASR 2.2 s spikes = whisper temperature fallback → disabled; max now 412 ms
- [x] Batching phrases rejected (no padding mask in Candle OpenCLIP → cos 0.963); sequential kept
- [ ] Heuristic decider over-triggers on repeated subject variants (sunflower → flower)
- [x] Mic device lookup can hang → 5 s timeout + clear error in debug window
- [ ] Stretch (P2): end-of-talk grid of shown images
- [ ] With real Jev: watch for over-triggering on variants of the same subject (sunflower → flower)
- [ ] Clean build from scratch before the demo (`cargo clean` then `CARGO_BUILD_JOBS=2 cargo build --release`, ~20 min)
