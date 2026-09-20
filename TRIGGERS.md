# What Live Slides listens for

A quick guide for presenters (top) and for whoever tunes it (bottom). Canvas mode, English only.

## For the presenter

| You want | Say something like | Notes |
|---|---|---|
| A photo | Just talk about it: "Penguins can't fly, but they're great swimmers." | From the photo library; if the library has nothing about it, one is drawn (about 2 s more). Logos, brands, charts and text are never drawn. |
| Swap the photo in focus | "Actually, make that the white rose instead." | A different variant of what's shown. |
| A company logo | "Let's put up the Google logo." / "a logo of the company called Google" / "let's make this presentation about Google" | From the logo library, by name. A brand that isn't in the library gets a plain card with its name (never a drawn logo). |
| An icon or a flag | "an icon for teamwork", "a database icon", "the flag of Canada" | From the icon library, by name and tags. If nothing fits, a plain card with the word. |
| Two photos together | "…a red rose and a white rose side by side." | |
| A chart | Say the numbers: "two hundred users… five hundred… fifteen hundred", "fifty percent are students…" | Only numbers you actually say get charted. |
| Fix a number | "Sorry, actually it was six hundred." / "Let's correct March from seventy to eighty." / "Make that ninety." | Fixes that one value; the rest of the chart stays. If you name no month, it is the one you just talked about. |
| Add a number | "And in April we hit ninety five." | Adds to the chart already on screen. |
| Take one bar / step out | "Drop February." / "Let's take the test step out." | |
| Rename or restyle | "Call this chart monthly signups." / "Show that as a line chart." | |
| A before → after | "Setup time dropped from twenty minutes to two minutes." | |
| A process diagram | "First we record audio. Then… Next… Finally…" | Grows one step per sentence; can start with one step. |
| A loop | "…and it all runs in a loop." | Turns the process into a cycle. |
| Parts of a whole | "The system is made up of three parts: the ears, the brain and the canvas." | |
| A timeline | "In 2019 we… In 2021 we… In 2023…" | |
| Structured text | "There are three lessons. First, start with users. Second, measure the outcome." | Explicit headings, lists, takeaways, concise headline claims and closings become text; ordinary narration does not. A single actor and outcome is text, not a two-node diagram. |
| Fix text | "Actually, make the second point measure the real outcome." | Updates that block without redrawing the card. |
| Compare / zoom / point | "Let's compare them side by side." "Zoom in on the owl." "Notice the eyes." | |
| Remove a picture | "Take the eagle away." / "Get rid of the chart." | Name the thing. |
| Clear the board | "Let's move on." / "Next topic." | Once per section. |

**Tips**
- The screen reacts to your newest words within about 2–3 seconds; a pause at the end of a sentence helps it settle.
- Say "fifty percent", not "half" (see *Numbers* below).
- Filler ("um, okay, where was I") and greetings show nothing, by design.
- Things not in the library can't be shown. Dev library: baseball, basketball, bowling, cactus, chalk, dahlia,
  dandelion, drum, eagle, earth, eight ball, flower, football, fortune cookie, gingerbread man, golf, guitar, hockey,
  leaf, lightning, lotus, medal, nest, owl, parrot, penguin, piano, poppy, red rose, sand dollar, smack, snowflake,
  soccer, sunflower, target, tennis, turntable, violin, white rose, yellow daisy, yin-yang, zebra, zen.
- Product names get misheard by the transcriber; list them in `talk-terms.txt` (or `TALK_TERMS`) to give it a hint.

## How each decision is made

**One decision-maker: Luna** (`gpt-5.6-luna` on OpenAI's own API, or through OpenRouter as `openai/gpt-5.6-luna`; `crates/agent`). Jev, the image-phrase model and
the stage's hold/confirm rules are gone from the runtime path. Everything that changes the screen — a photo, a chart,
a diagram, a correction, a removal, a layout change, a clear — is one Luna tool call.

Each call gets, in this order (the front of the message is stable between calls so providers can cache it):
1. **the transcript before the newest words**, oldest first, each sentence stamped with when it was said (capped at ~10k tokens). The new sentences are listed once, in part 4, not also here: showing them twice made "earlier speech" ambiguous;
2. **the board as it is now**, with tile ids (`e1`…), diagram node ids (`n1`…), chart points, focus, layout and limits;
3. **recent changes** — the last ten things done to the board, so it doesn't repeat itself;
4. **the newest words**: the sentences since its last call, and the phrase being spoken right now.

It acts only on the newest words; earlier speech is context. Its answer is tool calls, or `no_action(reason)`
(`tool_choice` is `required`, so "nothing to do" is an explicit, logged choice):

| Tools | For |
|---|---|
| `show_photo(subject, mode add\|replace)` | A photo of a real-world thing. The pipeline embeds Luna's phrase with CLIP, searches the library, and draws the picture if nothing matches. |
| `show_logo(name)` | A company / product logo, looked up **by name and alias** (no embeddings). Not found → a plain name card. |
| `show_icon(concept, alternatives)` | A generic icon or a country flag, looked up by name and tags. Luna also passes 2–4 synonyms ("teamwork" → users, group, people), because the library knows names like "users", not every idea. Not found → a plain card with the concept. |
| `show_logo` / `show_icon` accept `mode replace` | Swap the symbol just shown for a better one: the words were cut off ("the flag…" → "…of Canada") and Luna acted on the unfinished phrase. |
| `draw_chart` · `set_point` · `add_point` · `remove_point` · `set_chart` | A new chart; correct one value; add a point; drop a point; change kind / title / unit. Points are addressed by label ("Mar" finds "March"). |
| `draw_diagram` · `add_nodes` · `update_node` · `remove_node` · `add_edge` · `remove_edge` | A new diagram; grow it; rename a step; drop a step; link or unlink steps. |
| `draw_text` · `add_text_blocks` · `update_text_block` · `remove_text_block` | A structured heading/list/takeaway/closing; extend it; correct one block; remove one block. |
| `focus` · `arrange` · `annotate` · `clear_annotations` | Layout and emphasis. |
| `remove(id)` · `clear_board` | Take a tile away; clear the screen. |

**When Luna is called:** whenever it is idle, at least 3 new words have arrived (partial phrases count), and the
rate cap allows (`AGENT_RPM`: 500/min on OpenAI; 18/min on OpenRouter, where a new account is limited to 20/min).
Sentences that arrive while it is busy are merged into the next call, not dropped. After the audio ends, the last
words get a final call.

## Hard rules in code (the model judges the language; the code checks the evidence)

- **Destructive ops need a quote.** `remove`, `clear_board`, `remove_point`, `remove_node` and `remove_text_block` carry a `quote`: the
  exact words in which the presenter asked. The code checks every word of it appears, in order, in the *newest*
  words. No quote, or a quote from earlier speech, and the op is refused (and logged). There are no phrase lists
  any more, so "take the eagle away" and "let's park that" work as well as "remove". One clear per 6 s.
- **Chart values must be numbers the presenter said** (or already on the board). A number said with a scale word
  also grounds the bare number ("eighteen million" → 18, for a chart kept in millions). Invented remainders like
  "Not stoned: 40" are dropped.
- **Ops address things by id**, so an op still applies when a photo landed while Luna was thinking. If its target
  is gone, it is refused and logged. A `clear_board` clears only the tiles Luna saw.
- **Visible text waits for a finished sentence.** Luna still sees partial speech and can react quickly with other tools,
  but draw/add/update text calls from a partial-only turn are refused. Text blocks use the presenter's words; emphasis
  phrases must be exact substrings and are capped at two per block.
- Canvas limits: ≤ 4 tiles, ≤ 3 annotations, ≤ 8 nodes and ≤ 8 points; a stat with a second value becomes bars; a
  redraw sharing half its nodes with a diagram on the board replaces it in place.
- **Logos and icons never go to image generation.** They are looked up in `LS_ASSETS/icons` (`crates/search/src/icons.rs`, a port
  of `assets-pipeline/icon_search.py`, 63/63 queries identical to the Python on the real card). Logos are strict: an exact
  name or alias, or a fuzzy match ≥ 0.85 (a typo of a long name). A lone weak lookalike is only a candidate: "Hooli" once
  matched the "Hoodie" logo, and a wrong logo is worse than a name card. Icons and flags accept a slightly looser match.
  A `show_photo` whose subject says "logo" or "icon" is rerouted to the symbol tools. Monochrome icons are re-inked for
  the theme (`generated/symbols/`). A passing mention of a company ("Google announced…") shows nothing.
- **Photos:** a library match must clear the score floor `TAU` and, on the asset card, the label gate. A subject
  asked for twice within 25 s is one picture. A drawn picture is shown only if the subject is still in what was
  said (< 15 s old); logos, icons, charts, text and vague subjects are never drawn; drawn pictures are saved in
  `generated/` and reused.

Every refused op, every model failure and every "nothing to do" is in `logs/run-*.jsonl` (`ev: "agent"`, fields
`ops`, `applied`, `refused`, `no_action`, `error`).

## Diagrams and charts: technical diagrams, icons, and text that fits
**Technical diagrams** (architecture, data flow, request paths, pipelines): Luna draws a `flow` with an explicit edge for
EVERY connection the presenter describes (fan-out and fan-in included), each with a short label of what travels, and grows it
with `add_nodes` (with their edges) as more components are described; up to 10 nodes. The renderer lays it out in layers, orders
each layer to cut crossings, pulls each box toward the boxes it connects to, draws edges as curves between facing sides, and
writes edge labels in the gaps between columns (trying spots along the curve until one touches no box or other label).

**Icons and logos on nodes and chart points.** Luna can attach a picture to a node or point:
- `logo`: any NAMED product, technology or company ("Postgres", "Kafka", "Spark", "Snowflake", "React", "AWS"). Full-colour, from the logo library.
- `icon`: a generic thing, ONE name from a fixed list of 141 names Luna is given (`server`, `database`, `globe`, `network`, `lock`, `users`, …;
  `crates/agent/src/icon_names.txt`, each verified to be an exact icon in the library). Every generic icon comes from the same set
  (Lucide), so the style is consistent, and the same kind of thing always gets the same icon.
- The pipeline resolves these by name (`IconSearch::picture_for`) and is strict: a logo needs the model to name the product and an
  exact or strong match; a generic icon needs its whole name to match. Anything weaker gets **no picture** (a wrong picture is worse than none).
  Pictures appear on most nodes of a diagram or on none (≥ 60 % resolved, else all dropped); nodes added later match the diagram they join.
  On charts only a named product gets a logo (cloud providers, databases); labels are never turned into icons.

**Text fitting.** Every string is measured with the font it is drawn in (`app/dist/textfit.js`), never guessed from the character count. It
is wrapped by words, shrunk (down to a floor) before any word is split, and only cut with an ellipsis as a last resort. The whole diagram
gets one text scale: the largest at which every box holds its longest word and every gap holds its edge labels, so a crowded diagram
gets smaller text, not clipped text. Titles wrap to two lines; bar and line labels sit under the axis sized to their slot (with the picture
above them); a pie has a legend (colour chip, picture, name, share) so a label can never leave the tile or collide; labels drawn over lines
get a paper-coloured halo; line-chart values sit below a dip. Both themes use the same renderer: `sketch` (hand-drawn) and `slate` (clean
lines, solid fills, dark palette). Known limit: full-colour logos that are black in the library (Kafka, IBM) are hard to see on slate.

**Checking it.** `python3 scripts/preview_scene.py scripts/preview/text_cases.py <out-dir> --theme both` draws hard cases (long labels,
architecture with fan-out, logos, pie with 7 slices, small grid tiles) in headless Chrome with the real web view, saves PNGs, and reports
any text outside its tile or box, overlapping text, a label running into a node, or a split word. Exit status 1 on any problem.

## Backends: the two APIs are not the same shape
The same tools, prompt and parsing go to either; only the request differs (`CanvasAgent::request_body`, unit-tested):

| | OpenAI (`OPENAI_API_KEY`) | OpenRouter (`OPENROUTER_API_KEY`) |
|---|---|---|
| model id | `gpt-5.6-luna` | `openai/gpt-5.6-luna` |
| transport | Chat Completions by default; `CANVAS_TRANSPORT=websocket` uses a warmed, continued Responses connection with HTTP fallback | Chat Completions |
| token limit | `max_completion_tokens` (`max_tokens` is rejected) | `max_tokens` |
| reasoning | `reasoning_effort: "none"`. Function tools on chat completions require it (`minimal` is not a value for this model; the alternative is the Responses API) | `reasoning: {effort: "minimal"}` |
| routing | Standard tier by default; `CANVAS_SERVICE_TIER=fast` opts into Fast mode | `provider: {sort: "latency"}` |
| latency measured | 0.8–1.5 s per call, median ~0.95 s | 1.4–2.25 s, median ~1.9 s |

## Offline fallback (no key, or Luna unreachable)
A model call gets one retry (a timeout, a 429 or a 5xx), then the rules answer: `rule_ops`
([crates/canvas](crates/canvas/src/lib.rs)) maps "compare / side by side", "focus on / zoom in", "notice / look at
the", "let's move on / next topic" to layout ops, and a presenter cue ("here's…", "take a look at…", "picture
this…", `CUES` in [crates/query](crates/query/src/lib.rs)) followed by a library subject shows that subject.

## Known limits
- Speech → photo is about 2.7 s for a library photo and about 7 s for a drawn one (measured on the spoken test
  talk `fixtures/audio/luna-edit-talk.wav`). Most of it is Luna's ~1.4 s call plus waiting for the previous call
  and the rate cap.
- Nothing yet checks *how well the words were heard*: a mis-transcribed partial can make Luna ask for the wrong
  photo. Whisper's per-token confidence is exposed (`transcribe_detailed`) but not yet used.
- The transcript is capped at ~10k tokens; older sentences are dropped, not summarised.
- English only; accents and a noisy room raise transcription errors (sessions are recorded to `logs/*.wav` for tuning).

## Testing
- `cargo run -p ls-agent --bin ls-agent-probe -- --runs 3` — 42 speech → board cases against the real model (see
  [probes/luna/README.md](probes/luna/README.md); baseline before the refactor in `probes/luna/BASELINE.md`).
- `./target/release/ls-replay fixtures/audio/luna-edit-talk.wav` then `python3 scripts/e2e_check.py logs/run-….jsonl` —
  a spoken talk through Whisper → Luna → canvas, checked against nine board milestones.
