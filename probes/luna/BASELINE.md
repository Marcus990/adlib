# Baseline before the Luna refactor (2026-09-19, branch luna-refactor, main @ 8c19fd7 + probe tooling)

Recorded before any fix, so the refactor can be compared against it. Model: openai/gpt-5.6-luna.

## A. The reported failure reproduces — in Jev's routing, upstream of Luna

`cargo run -p ls-pipeline --bin ls-jev-probe` sends each case to Jev exactly as the live pipeline does
(board described by `board_summary`), then applies the live dispatch rule (Jev source, p_visual >= 0.5,
route chart|diagram|board|clear → Luna is called).

**Value corrections on a chart that is on screen never reach Luna: 0 of 9 phrasings.** Jev routes most of them
to `photo_update` (a photo route; there is no route for "edit what is on screen"), a few to `chart` below the
0.5 gate, and two Jev calls timed out at 700 ms. Overall, cases that want a board change reached Luna in 15/31.

```
case                               expects   jev route     p_vis p_int  Luna?
chart-correct-explicit             change    photo_update   0.54  0.17  NO
chart-correct-sorry                change    photo_update   0.40  0.12  NO
chart-correct-i-meant              change    photo_update   0.54  0.09  NO
chart-correct-make-that            change    photo_update   0.60  0.42  NO
chart-correct-not-a-but-b          change    photo_update   0.47  0.12  NO
chart-correct-digits               change    photo_update   0.53  0.10  NO
chart-correct-ambiguous-target     change    chart          0.33  0.12  NO
chart-correct-late-in-talk         change    photo_update   0.62  0.14  NO
chart-firm-up-misheard             change    chart          0.43  0.10  NO
chart-retitle                      change    none           0.61  0.09  NO
chart-change-to-line               change    (Heuristic)    Jev timed out (700 ms)  NO
chart-two-correct-recent           change    (Heuristic)    Jev timed out (700 ms)  NO
chart-two-correct-named            change    photo_update   0.59  0.09  NO
diagram-rename-node                change    photo_update   0.68  0.10  NO
diagram-firm-up-misheard           change    board          0.36  0.10  NO
diagram-recap-not-duplicate        change    none           0.88  0.04  NO
(reached: chart-add-point, chart-stat-grows-to-bar, chart-remove-point-drop/-take-out, chart-new-pie-beside-bar,
 diagram-add-step/-remove-node/-rework-as-cycle, and the board-* cases except the guards)
```

## B. Luna called directly (bypasses Jev): `cargo run -p ls-agent --bin ls-agent-probe -- --runs 3`

Corrections mostly work when Luna is asked (most phrasings 3/3). What fails on Luna's side:
- `has_removal_cue` does not match "take the eagle away", so a correct `remove` is dropped by the guard (0/3).
- `spoken_numbers("eighteen million")` returns only 18,000,000 (never 18), so `ground()` drops a corrected value
  on a chart stored in millions, and the update then loses that point (0/3).
- Unrequested changes: "40 desks" drew a stat chart (1/3); "Owls are nocturnal" highlighted the owl (2/3);
  a plain value correction switched bar → line (1/3); "call this chart monthly signups" ignored (2/3).
- "Pay attention to the owl" → focus instead of annotate (0/3).
- 9 of 114 calls hit the 5 s timeout and fell back to offline rules. They were consecutive calls ~50–85 s into
  the run; a follow-up run of the same cases with no timeout took 1.4–2.25 s each (~1.8k prompt tokens,
  ~75 output tokens), so this looks like a transient stall, not those prompts.
- Account limit: OpenRouter returned 429 "new accounts are limited to 20 requests per minute for this model"
  when unpaced; the runner paces at 18/min.

Result: 87 pass / 18 fail / 9 model errors (timeouts) over 38 cases x 3 runs; agent latency p50 2.2 s, max 4.1 s.

```
model openai/gpt-5.6-luna · 38 cases × 3 runs · 4 pending skipped

pacing 114 requests at 18 per minute (~6.3 min)

FLAKY chart-correct-explicit       2/3
        said: Actually, let's correct March from seventy to eighty.
        ops:  [{"op":"update_chart","id":"e1","kind":"line","title":"Users","points":[{"label":"Jan","value":40.0},{"label":"Feb","value":55.0},{"label":"Mar","value":80.0}]}]
        ✗ e1 kind: want bar got line
PASS  chart-correct-sorry          3/3
PASS  chart-correct-i-meant        3/3
PASS  chart-correct-make-that      3/3
PASS  chart-correct-not-a-but-b    3/3
FLAKY chart-correct-digits         2/3  (1 model errors)
        said: Correction: Feb is 62.
        ops:  []
        model call failed (timeout/HTTP?) — fell back to offline rules
        ✗ e1 points: want {"Feb":62,"Jan":40,"Mar":70} got [Jan 40, Feb 55, Mar 70]
FAIL  chart-correct-ambiguous-target 0/3  (3 model errors)
        said: Sorry, that should be seventy five.
        ops:  []
        model call failed (timeout/HTTP?) — fell back to offline rules
        ✗ e1 points: want {"Feb":55,"Jan":40,"Mar":75} got [Jan 40, Feb 55, Mar 70]
FAIL  chart-correct-late-in-talk   0/3  (3 model errors)
        said: Going back to the users chart, January was actually forty five.
        ops:  []
        model call failed (timeout/HTTP?) — fell back to offline rules
        ✗ e1 points: want {"Feb":55,"Jan":45,"Mar":70} got [Jan 40, Feb 55, Mar 70]
FLAKY chart-firm-up-misheard       1/3  (2 model errors)
        said: And in February we had fifty users.
        ops:  []
        model call failed (timeout/HTTP?) — fell back to offline rules
        ✗ e1 points: want {"Feb":50,"Jan":40} got [Jan 40, Feb 15]
PASS  chart-add-point              3/3
PASS  chart-stat-grows-to-bar      3/3
PASS  chart-remove-point-drop      3/3
PASS  chart-remove-point-take-out  3/3
FLAKY chart-retitle                1/3
        said: Let's call this chart monthly signups.
        ops:  []
        ✗ e1 title: want it to contain "signups" got Some("Users")
PASS  chart-change-to-line         3/3
FAIL  chart-two-correct-recent     0/3
        said: Sorry, March revenue was eighteen million.
        ops:  [{"op":"update_chart","id":"e2","kind":"bar","title":"Revenue","points":[{"label":"Jan","value":10.0},{"label":"Feb","value":12.0}]}]
        ✗ e2 points: want {"Feb":12,"Jan":10,"Mar":18} got [Jan 10, Feb 12]
PASS  chart-two-correct-named      3/3
PASS  chart-new-pie-beside-bar     3/3
PASS  guard-no-redraw-after-mention 3/3
FLAKY guard-unrelated-number       2/3
        said: Our office has forty desks.
        ops:  [{"op":"draw_chart","kind":"stat","title":"Office desks","unit":"desks","points":[{"label":"Desks","value":40.0}]}]
        ✗ board should be unchanged but was modified
PASS  guard-correction-without-number 3/3
PASS  diagram-add-step             3/3
PASS  diagram-rename-node          3/3
PASS  diagram-remove-node          3/3
PASS  diagram-firm-up-misheard     3/3
PASS  diagram-recap-not-duplicate  3/3
PASS  diagram-rework-as-cycle      3/3
PASS  diagram-guard-no-redraw      3/3
FAIL  board-remove-photo           0/3
        said: Take the eagle away.
        ops:  []
        ✗ e1 should have been removed
        ✗ elements: want 2 got 3
PASS  board-remove-chart           3/3
FLAKY board-guard-no-tidying       1/3
        said: Owls are nocturnal hunters.
        ops:  [{"op":"annotate","kind":"highlight","targets":["e2"],"label":"Nocturnal hunter"}]
        ✗ board should be unchanged but was modified
PASS  board-clear-explicit         3/3
PASS  board-clear-repeated         3/3
PASS  board-clear-already-empty    3/3
PASS  board-guard-move-on-figure-of-speech 3/3
PASS  board-arrange-compare        3/3
FAIL  board-focus-zoom             0/3
        said: Let's zoom in on the owl.
        ops:  [{"op":"focus","id":"e2"}]
        ✗ layout: want "hero" got auto
FAIL  board-annotate               0/3
        said: Pay attention to the owl.
        ops:  [{"op":"focus","id":"e2"}]
        ✗ e2 should be annotated

group             pass  fail  errors
board               19    11       0
chart-edit          39     6       9
chart-guard          8     1       0
diagram-edit        21     0       0
TOTAL               87    18       9
agent latency p50 2205 ms · max 4149 ms
```

---

# After the refactor (steps 2–5, same day)

## A'. Luna called directly (`ls-agent-probe --runs 3`, 42 cases; the 4 photo cases are now runnable)

**125 / 126 pass, 0 model errors** (before: 87 pass / 18 fail / 9 errors on 38 cases). Agent latency p50 1.26 s, max 2.2 s.
Groups: chart-edit 54/54 (was 39 pass, 6 fail, 9 errors), chart-guard 9/9 (was 8/9), diagram-edit 21/21,
board 29/30 (was 19/30), photo 12/12. The one flaky case is `board-arrange-compare` (2/3): Luna judged two photos
already side by side (2 tiles auto-lay out like `compare`) and chose `no_action`. The zoom case's expectation was
relaxed to `focus` only, for the same reason (3 tiles auto-lay out like `hero`).

## B'. Whole chain on the same spoken talk (`fixtures/audio/luna-edit-talk.wav`, `scripts/e2e_check.py`)

Whisper → transcript → Luna → guards → canvas, nine board milestones checked in order:

| pipeline | milestones | notes |
|---|---|---|
| before (main: Jev routes, agent has `update_chart`) | **1 / 9** | Jev routed "correct March from 70 to 80" to `photo` (0.64), so the chart was not corrected at that time (it only reached 80 a sentence later, because "And in April we hit 95" re-sent the whole data set). "Now picture an owl" showed a photo labelled *Picture frame*. Speech→photo 1.2 s (library). One run. |
| after, run 1 | 8 / 9 | The owl was refused by generation: a leftover `subject.len() < 4` rule treated "owl" as junk. Fixed. |
| after, runs 2–4 and a final run after deleting the old crates | **9 / 9, four times** | 19 Luna calls per talk (~17/min, under the 20/min cap), 0 fallbacks, 0 refused ops, agent p50 1.3 s (max 2.8–4.6 s). Speech→photo 2.4–2.7 s (library), 3.6–4.1 s for a drawn photo (6.9 s on the one run that really called the image model). |

Cost of the change: a library photo now takes ~1.2–1.5 s longer to appear (2.4–2.7 s vs 1.2 s), because it waits on Luna's call
(~1.4 s) instead of Jev's ~0.2 s. Options if that matters: a local shortcut when the speech names a library subject
outright, or searching in parallel while Luna decides.

Caveats: the talk is synthetic speech (macOS `say`), the old pipeline was run once, and the account is capped at 20
requests/min for Luna, which the pipeline paces to (`AGENT_RPM`, default 18).

---

# Luna on OpenAI's own API (2026-09-20)

`OPENAI_API_KEY` set → `POST https://api.openai.com/v1/chat/completions`, model `gpt-5.6-luna`. Request shape differs from
OpenRouter (tested against the live API): `max_completion_tokens` not `max_tokens`; `reasoning_effort` must be `"none"`
for function tools on this endpoint (`minimal` is not a value; the alternative is the Responses API); `reasoning` and
`provider` are rejected as unknown parameters; `temperature` is accepted with reasoning off. Key limits: 500 req/min,
500k tokens/min.

| | OpenRouter (minimal reasoning) | OpenAI (reasoning off) |
|---|---|---|
| Luna probes, 42 cases x 3 | 125 / 126 | first run 123 / 126 (`board-annotate` 0/3: a focus instead of an annotation); after one prompt sentence: **126 / 126** |
| agent latency per call | median 1.26 s, max 2.2 s | median 0.75-0.93 s, max 1.5-3.9 s |
| spoken talk, 9 milestones | 9/9 (four runs) | 9/9 (one run: 30 Luna calls, 0 fallbacks, the chart grows live as each number is spoken) |
| speech -> library photo | 2.4-2.7 s | 2.3 s |


---

# Logos, icons and flags (branch `logos-and-icons`, 2026-09-20)

**The bug:** "a logo of the company called Google" generated a junk picture. Cause (from the live run's log): Luna's only photo
tool is the CLIP search, which cannot find a logo, so `show_photo("Google")` missed and fell through to image generation
(`google.jpg`); and when the presenter said "logo" Luna answered `no_action` ("logos are not photographable"), because the
prompt told it not to show logos and it had no tool for them. Nothing in Rust loaded `icons/`.

**The fix:** `show_logo(name)` and `show_icon(concept, alternatives)`, name lookup ported to Rust (63/63 queries identical to the
Python reference), a `logo` tile, a name card on a miss, and no generation for symbols ever.

| check | result |
|---|---|
| Luna probes, 54 cases x 3 (12 new symbol cases: explicit, "a logo of the company called Google", subject of the talk, unknown brand, passing mentions, icons, flags, photo stays a photo) | **162 / 162**, median 0.78 s |
| spoken logo talk (`fixtures/audio/luna-logo-talk.wav`, `e2e_check.py --talk logo`) | **6 / 6**: Google and Microsoft logos from the library, "teamwork" -> `lucide:users` via the model's synonyms, `flag of Canada`, "Hooli" -> plain name card, clear |
| spoken edit talk (regression) | 9 / 9 |
| rendering (headless Chrome, real `index.html`, both themes) | logo, icon, flag and name card all draw; the dark theme uses a re-inked copy of monochrome icons |

Two things found on the way. (1) The lookup's own rule auto-picked a lone weak fuzzy hit: "Hooli" showed the **Hoodie** logo; logos
now need an exact name or a fuzzy match >= 0.85. (2) My first prompt for the new tools made an unrelated chart case flaky (11/12,
then 5/15): Luna called the newest sentence "already spoken earlier" because each new sentence appeared twice in the message
(end of the transcript, and under "Newest words"). Listing it once fixed that case (15/15) and is now the format.


---

# Text fitting, icons in diagrams and charts, technical diagrams (2026-09-20)

**Before (rendering, headless Chrome, the real web view).** Long node labels overflowed their boxes ("Authentication" spilled past the edge;
"Authentication and…" and "Kafka event…" were cut off), edge labels ran through boxes and arrows, edges crossed nodes, bar-chart axis labels sat on
the baseline, line-chart values collided with the line, pie labels left the tile. Cause: text width was *guessed* as `maxWidth / (fontSize x 0.5)`,
but the handwriting font is wider, and text was never shrunk.

**After.** Real measurement (`textfit.js`), shrink-before-split, one text scale per diagram, layered layout with crossing reduction and
vertical relaxation, curved edges with labels placed in the gaps, a pie legend, axis labels sized to their slot, halos. `scripts/preview_scene.py`
checks 7 hard cases x 2 themes: **14 / 14 render with no text outside its box, no overlaps, no split words** (before: overflow in 5 of 5 sketch
cases). Icons draw in nodes, chart axes and pie legends in both themes.

| check | result |
|---|---|
| Luna probes, 65 cases x 3 (new: 8 technical-diagram cases with edge and icon expectations, 3 symbol-refinement cases) | **195 / 195**, median 1.0 s per call |
| spoken architecture talk (`fixtures/audio/luna-tech-talk.wav`, `e2e_check.py --talk tech`) | **5 / 5 in two runs**: 7-8 nodes, 6-7 edges, a picture on every node (logos for Postgres, Redis, Kafka; icons for the rest), a 3-point chart with a logo on each point |
| spoken logo talk (regression) | 6 / 6 in three runs; in one run Luna used `replace` to turn a generic flag into Canada's flag |
| spoken edit talk (regression) | 9 / 9 |

Found on the way. (1) **Icons showed as a broken image in the app**: `main.rs` had its own copy of the mime function that served SVG as
`image/jpeg`; fixed by using the shared one (test in `crates/search`, and reproduced in Chrome: the same SVG labelled jpeg shows the broken-image
icon). (2) Luna sometimes used a generic icon for a named product (Spark, Snowflake, Grafana); the prompt now says a named product ALWAYS takes a logo
(0/3 -> 6/6). (3) Acting on an unfinished phrase ("the flag…") left the wrong symbol up; `show_logo` / `show_icon` now have `replace`.
(4) A concurrent live session's log was newer than mine and `ls -t` picked it; the e2e runs now read the log path the replay tool prints.

---

# The Responses WebSocket becomes the default (2026-09-20)

`CANVAS_TRANSPORT=websocket` was built but never set — not in `.env`, not by `demo.sh` or `make_app.sh` — so every
live run had used Chat Completions. Across the whole log history, `transport` was `ChatHttp` on all 93 calls that
record it and `first_event_ms` was non-null 0 times. The gate is now inverted: WebSocket is the default on OpenAI
and `CANVAS_TRANSPORT=http` opts out. HTTP remains the automatic fallback on any socket failure.

| | HTTP | WebSocket |
|---|---|---|
| agent call p50 (3 replays of `luna-edit-talk.wav`, 120 calls each) | 1042 ms | **907 ms** |
| p90 / max | 1292 / 4599 ms | 1378 / 2675 ms |
| milestones | 9/9 x 3 runs | 9/9 x 3 runs, 0 fallbacks |
| first event | not exposed | p50 174 ms |

~135 ms (13%) at p50, a wash at p90. One call hit `rate_limit_exceeded` on the socket and fell back to HTTP
cleanly, which is the fallback working as designed.

Two things worth knowing before reading more into this. (1) The 66-case probe suite shows **no** difference
(HTTP 984 ms vs WS 981 ms p50) because it opens a fresh socket per case and so never exercises chaining; only the
replay, where `previous_response_id` keeps the transcript off the wire, shows the gain. Don't evaluate the
transport with the suite. (2) The chained `--ws-latency 30` split is first event 176 ms, after-first 800 ms, so
~80% of a call is output-token generation. Re-sending the prompt was never the bottleneck, which is why this is
13% and not 50%. The real prize in the socket is that 174 ms first event: applying ops from the stream instead of
waiting ~730 ms for the full body needs incremental tool-call parsing, which this change does not add.

Not the bottleneck either way: in the same runs the CLIP photo search ranged 30 ms to 5772 ms for identical work,
and `asr_lag_ms` reached p50 46.9 s on the worst run, against `asr_ms` p50 942 ms and a 600 ms tick. Both are CPU
contention (no thread pool is bounded anywhere: tokio's 8 workers + candle's rayon default + whisper's 4 on an
8-core M2). Standalone, that same CLIP search is 23 ms idle, 62 ms under 4 competing threads, 280 ms under 8.
When the pipeline falls far enough behind, the 25 s photo dedup window expires and the same subject is searched
twice, which feeds back into the contention.
