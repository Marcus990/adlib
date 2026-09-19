# TODO

## Setup
- [x] Create repo, workspace, contracts crate (source written)
- [ ] `cargo test -p ls-contracts` (needs crates.io download permission)
- [ ] Install/verify `a` (github.com/Marcus990/a) — needs download permission
- [ ] `.env.example` with JEV_API_KEY, GEMINI_API_KEY

## Phase 0 spikes (measure, record in PROGRESS.md)
- [ ] S1 Jev: access, request shape (JS SDK), 20 timed calls
- [ ] S2 Query model: Gemini 2.5 Flash-Lite, 20 timed calls, phrase quality
- [ ] S3 whisper-rs + Silero VAD latency (base.en vs small.en) on this M2 8 GB
- [ ] S4 Candle MobileCLIP: load, embed 1 image + 1 sentence, timing

## Tracks
- [ ] A Hear: capture, VAD, whisper, Chunk events, WAV replay, JSONL, fixtures
- [ ] B Search: indexer, captions, scoring (§7.3), best_match, LRU, prefetch, img://
- [ ] C Decide: Jev client, state builder, ChangeDecision, fixture replay test
- [ ] D Query: client, JSON, 700 ms timeout, noun-phrase fallback, tests
- [ ] E Stage: join + state machine (§7.4/7.5), unit tests
- [ ] E Show: Tauri app, full-screen, crossfades, blurred backdrop, clear

## Integration (in order)
- [ ] hand-made Chunk → Show
- [ ] Search → Show
- [ ] Query → Search → Show
- [ ] Decide + Query/Search → Join → Show
- [ ] WAV replay → full pipeline
- [ ] live mic → full pipeline
- [ ] demo library; rehearsals ×3
