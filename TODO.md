# TODO

Owner: **me** = coding agent can do it now · **you** = needs the human · blocked items say what they wait on.
Done items live in PROGRESS.md / git history.

## A. Display-intent redesign (user idea, 2026-09-19) — P0
Jev should answer "is the presenter signalling that the audience should SEE something?", not "was something
picturable mentioned?". "I like watermelons" → no change; "here's what a watermelon looks like" → show.
- [ ] A1 (me, S) `crates/decide`: ask Jev two questions in one call — `intent` (noul: presentational signal to
      show a picture/graphic/chart now?) and `kind` (choice: new_render / update / clear). Action =
      `kind` if P(intent) ≥ τ_intent (start 0.6), else `no_change`; `p` = P(intent)·P(kind). ChangeDecision
      contract unchanged. Keep the old topic-shift prompt behind `DECIDE_MODE=topic` for A/B.
- [ ] A2 (me, S) Offline heuristic: require a presentational cue ("here's", "take a look", "look at", "picture this",
      "this is what", "as you can see", "imagine", "let me show you", "check out") near a library subject in the
      newest words; a bare mention is `no_change`.
- [ ] A3 (me, XS) Query-model prompt: when a cue is present, phrase the object after the cue ("here's what a
      watermelon looks like" → "watermelon").
- [ ] A4 (me, XS) Mock OpenRouter: same intent behaviour, so the hosted-path replay tests it.
- [ ] A5 (me, S) Fixtures + eval: new rehearsal talk with cue phrases AND non-intent mentions; `expected.tsv`
      gains negative rows (`watermelon<TAB>-`) and `eval_run.py` reports false positives, not just hits.
- [ ] A6 (me, XS) Talk-writing guide in README: the presenter must *say* the cue ("here's…", "take a look…");
      one cue per image; list of cues that work.
- [ ] A7 (both, blocked: api key) Tune τ_intent and Jev wording on real Jev answers; watch for under-triggering.
- [x] A8 Charts: done differently — live charts drawn from spoken numbers (CANVAS.md, 09-19).

## B. Pipeline — reliability & latency
- [ ] B1 (me, S, P0) Mic-drop watchdog: if no audio blocks for 2 s mid-talk (AirPods disconnect / route switch),
      re-open capture (same device → built-in mic), show it in the debug window. Today the loop just waits.
- [x] B2 (me, XS, P1) Whisper initial prompt with library captions (done 09-19; talk terms still to add).
- [ ] B3 (me, S, P1) ASR tick 750 → 500 ms if CPU/Metal headroom allows (measure first).
- [ ] B4 (me, M, P2) Local-first search: show local-phrase match immediately, upgrade if remote phrases beat it
      within the join window (takes the ~435 ms query model off the critical path).
- [ ] B5 (me, XS, P1) `caffeinate` in demo.sh / .app so the Mac can't sleep mid-talk.
- [ ] B6 (both, blocked: api key) Real-model measurement: `./scripts/morning_check.sh AirPods`; bake-off
      Gemini 2.5 vs 3.x Flash-Lite on latency + phrase quality; lock the model.

## C. Data
- [ ] C1 (me, XS, P0) Index portability: store paths relative to index.json (today `root` is absolute → moving
      the SD card / Mac breaks it).
- [ ] C2 (me, S, P1) Caption aliases + exact-term boost: optional `aliases` column in captions.tsv
      ("Tokyo office, HQ Japan"); verbatim alias hits raise the score — MobileCLIP doesn't know company terms.
- [ ] C3 (me, S, P1) Gap report: `scripts/gaps.py <log>` lists every "change wanted, no image good enough" with
      what was said → tells you which images to add.
- [ ] C4 (you, blocked: library) Build the demo library (50–150 images, ≥1920 px, 16:9 preferred, deliberate
      variants for refinements) → `prepare_library.sh` → edit captions → `ls-index`.
- [ ] C5 (both) Calibration set for your library (30+ phrase→image, 10+ phrase→none) → `ls-calibrate` → set TAU.
- [ ] C6 (you) Write the 3-minute talk with display cues (see A6) + its expected.tsv.

## D. App & demo-day UX
- [ ] D1 (you, P0) Approve macOS mic permission for "Live Slides" (first launch of build/Live Slides.app).
- [ ] D2 (you, P0) Connect AirPods; test on the real projector with `LS_DISPLAY` (untested on a 2nd display).
- [ ] D3 (me, XS, P1) Debug window: show Jev intent probability + a thumbnail of the *held* (pending) image.
- [ ] D4 (me, XS, P2) Optional auto-grid on a spoken closing cue ("thank you", "to wrap up") in live mode.
- [ ] D5 (me, S, P1) DEMO.md run-sheet: DND on, volume/mic check, `morning_check`, launch, fallback video, what
      to do if the image is wrong (`b` blank) or the mic drops.

## F. Diagrams & charts (09-19)
- [ ] F1 (me, S) Mock OpenRouter: emit draw_chart / draw_diagram tool calls so offline replays exercise graphics.
- [ ] F2 (me, S) eval: score graphics (expected chart values / diagram node count per fixture).
- [ ] F3 (you) Say numbers and steps in the demo talk on purpose — they are what triggers graphics.

## E. Tests & regression
- [ ] E1 (me, S, P1) `scripts/regress.sh`: one command — mock OpenRouter + headless replay of every fixture +
      eval with pass/fail thresholds (renders correct, zero false positives, p50 latency).
- [ ] E2 (me, S, P2) Pipeline crate integration test with fake ASR/decider/query (today only the pure crates
      have tests).
- [ ] E3 (you → me) Record 3 real rehearsals on AirPods (WAV) → add as regression fixtures with expected.tsv.
- [ ] E4 (both) Three clean live rehearsals end to end = demo gate (design doc §8).
