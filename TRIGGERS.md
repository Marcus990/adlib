# What Live Slides listens for

A quick guide for presenters (top) and for whoever tunes it (bottom). Canvas mode, English only.

## For the presenter

| You want | Say something like | Notes |
|---|---|---|
| A photo | Just talk about it: "Penguins can't fly, but they're great swimmers." | Naming a library subject outright is fastest (≈0.2–0.4 s after the word). It must be in the library. |
| Swap the photo in focus | "Actually, make that the white rose instead." | A different variant of what's shown. |
| Two photos together | "…a red rose and a white rose side by side." | |
| A chart | Say the numbers: "two hundred users… five hundred… fifteen hundred", "fifty percent are students…" | Only numbers you actually say get charted. |
| Fix a number | "Sorry, actually it was six hundred." | |
| A before → after | "Setup time dropped from twenty minutes to two minutes." | |
| A process diagram | "First we record audio. Then… Next… Finally…" | Grows one step per sentence; can start with one step. |
| A loop | "…and it all runs in a loop." | Turns the process into a cycle. |
| Parts of a whole | "The system is made up of three parts: the ears, the brain and the canvas." | |
| A timeline | "In 2019 we… In 2021 we… In 2023…" | |
| Compare / zoom / point | "Let's compare them side by side." "Zoom in on the owl." "Notice the eyes." | |
| Remove one thing | "Let's remove the eagle." / "Get rid of the white rose." | Name the thing. |
| Clear the board | "Let's move on." / "Next topic." | Once per section. |

**Tips**
- Pause a beat at the end of a sentence — charts, diagrams and board commands are decided when a sentence finishes.
- Say "fifty percent", not "half" (see *Numbers* below).
- Filler ("um, okay, where was I") and greetings show nothing, by design.
- Things not in the library can't be shown. Dev library: baseball, basketball, bowling, cactus, chalk, dahlia,
  dandelion, drum, eagle, earth, eight ball, flower, football, fortune cookie, gingerbread man, golf, guitar, hockey,
  leaf, lightning, lotus, medal, nest, owl, parrot, penguin, piano, poppy, red rose, sand dollar, smack, snowflake,
  soccer, sunflower, target, tennis, turntable, violin, white rose, yellow daisy, yin-yang, zebra, zen.
- Product names get misheard by the transcriber (Jev → "JET", Tauri → "Tori"); a vocabulary hint is not in yet.

## How each decision is made

| Visual | Who decides | When it's considered | Hard rules in code |
|---|---|---|---|
| Photo | **Jev** decides *whether* ("is the talk about something picturable that isn't already on screen?", sees the whole board); the **phrase model** or the **named-subject shortcut** decides *what*; **image search** finds it | Every transcript update (~0.6 s) | Match score ≥ 0.52; Jev confidence ≥ 0.45 new / 0.4 swap / 0.7 clear; an unfinished phrase with Jev < 0.6 needs a second agreeing update; ≥ 1.5 s between new photos |
| Chart, diagram, layout, highlight | **Canvas agent** (Claude Haiku 4.5, tool calls) | **Every finished phrase (≥ 3 words)**, plus early mid-sentence on the trigger words below | Chart values must be spoken numbers (or already on the board); a new set of numbers or a bar → pie switch makes a *new* chart; same-layout redraw replaces the diagram in focus; ≤ 4 tiles, ≤ 8 nodes/points |
| Clear the board | Canvas agent | as above | Only if your newest words contain a *section* phrase; one clear per 6 s |
| Remove one tile | Canvas agent | as above | Only if your newest words contain a *removal* phrase |

Everything in the "who decides" column is a model judgement; everything in the last column and the lists
below is **hardcoded** (English phrases in Rust).

## The word lists (hardcoded)

**Mid-sentence graphics shortcut** — `has_graphic_cue`, [crates/canvas/src/lib.rs:688](crates/canvas/src/lib.rs):
any digit; percent, hundred, thousand, million, billion, half of, a third, a quarter, doubled, tripled, twice as,
grew, growth, increased, decreased, dropped, went up, went down, "first,", first we/you/the, then we/you/the/it,
after that, "next,", finally, step, stages, phase, process, pipeline, workflow, cycle, loop, leads to, results in,
feeds into, which means, because of, timeline, over the years, made up of, consists of, breaks down, "parts:".
*Only makes graphics start earlier; the finished sentence is always sent anyway.*

**Mid-sentence layout shortcut** — `has_layout_cue`, [crates/canvas/src/lib.rs:751](crates/canvas/src/lib.rs)
(only when something is on the board): compare, versus, vs, side by side, next to each other, both of these,
focus on, zoom in, this one, closer look, all of these, all together, altogether, notice, see how, look at the,
pay attention, connects to, leads to, compared to, just like, moving on, let's move on, next topic, new section,
set that aside.

**Clear permission** — `has_section_cue`, [crates/canvas/src/lib.rs:663](crates/canvas/src/lib.rs):
move on, moving on, next topic, new section, set that aside, start fresh, switch gears, switching gears, next up,
clean slate, clear the screen, change of topic, different topic.

**Remove permission** — `has_removal_cue`, [crates/canvas/src/lib.rs:675](crates/canvas/src/lib.rs):
remove, get rid of, take away, take that/it away, take that/it down, put that/it aside, set aside, set that aside,
forget the, forget about, drop the, hide the, don't need the, no longer need.

**Numbers** — `spoken_numbers`, [crates/agent/src/lib.rs:239](crates/agent/src/lib.rs): digits (with commas,
decimals, %, $), number words zero–nineteen, twenty–ninety, hundred / thousand / million / billion ("fifteen
hundred", "forty thousand", "a thousand", "1.2 million"). **Not understood:** half, a third, dozen, couple —
a chart value built from those is dropped.

**Named subjects** — `named_subject`, [crates/query/src/lib.rs:184](crates/query/src/lib.rs): not a fixed list —
the library's own captions. A phrase that names exactly one library subject is searched immediately; two subjects
("owls and penguins") or an ambiguous one ("a rose and a white rose") go through the phrase model.

**Offline fallback only** (no API key / model down) — `CUES` and `REFINE_OBJECT_CUES`,
[crates/query/src/lib.rs:140](crates/query/src/lib.rs): here's, take a look, look at, picture this, imagine, as you
can see, let me show you, check out, this is what…; make that, make it, switch to, change it to, instead of that,
the other one.

## Known limits
- Clear and remove need the listed phrases; paraphrases ("let's park that", "OK, new topic") won't clear/remove.
  Possible next step: ask Jev "is the presenter asking to change the board?" instead of matching words.
- English only; accents and a noisy room raise transcription errors (sessions are recorded to `logs/*.wav` for tuning).
