# Acceptance status (design doc §1 "What success looks like")

Evidence is from synthesized-speech rehearsals (`say` → WAV → real Whisper/VAD/MobileCLIP/stage/Tauri),
run unattended on 2026-09-19. **Hosted models (Jev, query model) have NOT been exercised against a
real key**, and the **live AirPods path has not been run** — both need the user (see bottom).

| Demo check | Target | Status | Evidence |
| --- | --- | --- | --- |
| First visual after first sentence | ≤ 2 s | Met (replay) | First subject ("bald eagle") rendered 47–50 ms after the keyword reached the transcript |
| Phrase end → new visual | ~1–1.5 s typical, ≤ 2.5 s worst | Met offline and with **simulated** hosted latency; real hosted models unmeasured | offline: keyword-in-transcript→render p50 70 ms headless / 175 ms in-app. Mock OpenRouter with vendor-like latency (Jev 70–500 ms, chat 300–550 ms): p50 428 ms; with 20% errors + 10% stalls: p50 469 ms. Add ≤ 0.75 s ASR tick ⇒ ~0.4–1.2 s from the spoken word |
| Visual changes in 3 min | 8–12, each about what was said | Met (replay) | 125 s rehearsal: 15 changes (13 subjects + 2 recap callbacks), 15/15 correct |
| No wrong/jarring visuals | 0 in final 3 runs | Met in 4 replays (headless offline, headless degraded, app) | 0 wrong renders; min gap between changes 4.01 s (hold works) |
| Stale decisions never overwrite newer | — | Met | stage unit tests (seq newest-wins, pending slot, join timeout) |
| Query fallback works | — | Met | invalid key → real 401s → fallbacks, 15/15; fault-injected mock (500s + stalls) → 15/15 after the decide time-budget fix |
| Replay works | — | Met | `ls-replay`, `LS_SOURCE=wav:` in the app, `scripts/eval_run.py` scoring |
| Logs explain timing | — | Met | JSONL: chunk (asr/vad/lag), decide (source/ms), search (query/search ms, phrases, score), join outcome, render (speech→render), frontend received/decoded/painted |
| Image cache works | — | Met | 43/43 prefetched; img:// decode 4–146 ms; cache unit test |
| Full-screen + crossfades | — | Built; **needs a human eyeball** | `LS_FULLSCREEN=1` / `f` key; frontend painted steps logged; screen capture impossible while display slept |
| Survives a 3-min run | — | Met (replay, app) | 125 s app run, no errors, memory < 3.2 GB guard |
| Live capture → full loop | — | Met with a live CoreAudio input (BlackHole ← `say`); AirPods + real hosted models pending | app, `LS_SOURCE=mic:BlackHole`: 15/15 correct, 15/15 painted, p50 71 ms |

Tools for the human steps: `scripts/prepare_library.sh` (images → jpg + editable captions), `ls-index`,
`ls-calibrate` (top-1 accuracy + best τ from labelled phrases), `scripts/eval_run.py`, and
**`scripts/morning_check.sh`** (key present? mic audio arrives? hosted-model rehearsal scored).
Stretch goal done: end-of-talk grid of every image shown (`g`, automatic when a replay ends).

## Needs the user (genuine blockers)
1. **`OPENROUTER_API_KEY` in `.env`** → rerun `./target/release/ls-replay fixtures/audio/rehearsal-3min.wav`
   and score with `scripts/eval_run.py`; check `decide.source == "Jev"`, Jev/query latency in the log.
   Model choice (Gemini 2.5 Flash-Lite vs 3.x Flash-Lite) should be settled on that measurement.
2. **Live mic session** (CoreAudio device lookup blocks for this unattended process — every device,
   5 s timeout — consistent with a pending microphone-permission approval): connect AirPods, run `LS_SOURCE=mic:AirPods ./target/release/live-slides`,
   approve the macOS microphone prompt. (Default input on this Mac is BlackHole — pick the mic by name.)
3. **Demo library + talk** (humans, design doc §8 hours 2–8): index with `ls-index`, recalibrate τ with
   `ls-search` (dev-library margin was only 0.009), write `expected.tsv` for the talk, rehearse ×3.
4. Eyeball the stage once (fit vs cover, blur backdrop, crossfade feel).
