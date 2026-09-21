# Adlib

**Won $10k cash and Semi-finalist at **Hack the North 2026**. Won the **Rox Best AI Agent** prize track.**

**The best way to present new ideas on the spot. No more slides you follow. Adlib follows you.**

Adlib is always listening to your voice. As you speak, it creates flow charts, diagrams, graphics and graphs live
on screen: charts from the numbers you say, flow diagrams from the steps you describe, photos, logos and icons for
the things you mention, and structured text for your key points. Change your mind mid-sentence ("sorry, it was
forty-eight percent, not forty-six") and the graphic edits itself in place. No slides, no clicking, no prompting.
You just talk.

## What it does

| You say | The screen |
|---|---|
| "Forty-six percent had nobody to go with, thirty-two percent didn't know where to start…" | Draws a hand-sketched bar or pie chart from the numbers you actually said |
| "Sorry, that first number was forty-eight." | Changes that one bar. The rest of the chart stays |
| "In week five we hit one hundred three." | Adds a point to the chart already on screen |
| "First we record audio, then transcribe it, then decide what to draw." | Grows a flow diagram one step per sentence |
| "…and it all runs in a loop." | Turns the flow into a cycle |
| "The API talks to Postgres and Kafka, and Kafka feeds Spark." | Draws an architecture diagram with a real logo on each node |
| "We wrote the backend in Python." | Puts up the Python logo |
| "There are three lessons. First…" | Writes a heading, paragraph and bullet card with the key phrases underlined |
| "Penguins can't fly but they're great swimmers." | Shows a penguin photo (or draws one if the library has none) |
| "Let's compare those. Zoom in on the owl. Notice the eyes." | Rearranges, zooms and circles |
| "Take the eagle away." / "Let's move on." | Removes a tile or clears the board |

Highlights:

- **Live and editable.** It doesn't only generate. It revises. Corrections, additions, renames and removals
  change the existing chart, diagram or text instead of redrawing it.
- **Hand-drawn look.** Charts and diagrams are sketched on paper (wobbly strokes that draw themselves in,
  taped-on polaroid photos, handwriting). A clean dark `slate` theme is also available.
- **Technical diagrams.** Layered auto-layout, crossing reduction, labelled edges, and text that is measured and
  fitted so nothing clips or collides.
- **Logos, icons and flags by name.** 13k SVGs looked up by name and alias. A wrong logo is worse than none,
  so a weak match becomes a plain name card.
- **Photos.** Semantic search over ~39k photos with CLIP. If nothing matches, SDXL-Lightning draws one in about 2 s.
- **Guardrails.** Chart values must be numbers you said out loud. Destructive commands (remove, clear) must be
  quoted from your newest words, so the model can't wipe the board on its own.
- **Works offline-ish.** Speech recognition and photo search run on-device. Without an LLM key, a small set of
  rules still handles layout cues and photos.
- **Fast.** Roughly 1–3 s from spoken sentence to graphic. A photo takes about 2.7 s, and graphics land 1.1–2.7 s
  after the sentence.

## How it works

```
mic ─▶ VAD (Silero) ─▶ Whisper (local, Metal) ─▶ live transcript
                                                      │
              transcript + current board + recent changes + newest words
                                                      ▼
                            Luna (GPT-5.6 Luna, Responses WebSocket)
                              one decision-maker, answers with tool calls
                                                      ▼
   draw_chart · set_point · draw_diagram · add_nodes · draw_text · show_photo · show_logo · show_icon
   focus · arrange · annotate · remove · clear_board · no_action
                                                      ▼
        guards (spoken numbers only, quoted removals) ─▶ Canvas: pure, deterministic board state
                                                      ▼
   show_photo ─▶ CLIP search ─▶ else SDXL-Lightning (Baseten)      show_logo / show_icon ─▶ name lookup
                                                      ▼
                     Tauri window renders the scene as SVG (sketch.js), animating only what changed
```

1. **Hear.** A chunker runs voice-activity detection and re-transcribes the in-progress sentence on a 0.6 s tick,
   so the model sees words while you are still saying them.
2. **Decide.** One agent, Luna, sees the whole transcript, the board with stable ids, and what it changed
   recently, and answers with tool calls. It changes nothing when there is nothing to do. Every turn is chained
   over a persistent WebSocket, so only the new words are sent.
3. **Apply.** The Rust canvas validates each op, applies it by id (so late answers still land correctly), and
   emits a new scene. The board holds up to 4 tiles: charts, diagrams, photos, logos and one text card.
4. **Draw.** The web view renders the scene as hand-drawn SVG. Strokes draw themselves, and only new nodes,
   bars and points animate.

Every run writes a JSONL log with timings, model decisions, refused ops and the board after each change.

## Tech stack

- **Rust workspace**, one crate per stage: `hear`, `agent`, `canvas`, `search`, `gen`, `pipeline`, `contracts`
- **Tauri 2** desktop app (stage window plus a debug window with live transcript and decisions)
- **Whisper** (`whisper-rs`, Metal) and **Silero VAD** for on-device speech recognition
- **Luna** (OpenAI `gpt-5.6-luna`, or via OpenRouter) for all on-screen decisions, with tool calling
- **CLIP ViT-B/32** through **Candle** for photo search (MobileCLIP-S2 for small local libraries)
- **SDXL-Lightning** on **Baseten** as the drawn-image fallback
- **Vanilla JS + SVG** for the sketch renderer, with in-house text fitting and layered diagram layout
- **Iconify** sets for logos, icons and flags. Open Images and COCO for photos
- **Python** for the asset pipeline (`assets-pipeline/`) and preview and test scripts

## Run it

Requires a Mac (Apple Silicon) with Rust and a microphone.

```bash
cp .env.example .env            # set OPENAI_API_KEY (or OPENROUTER_API_KEY); BASETEN_API_KEY is optional
CARGO_BUILD_JOBS=2 cargo build --release
./scripts/make_app.sh           # builds build/Live Slides.app
```

You also need the Whisper model (`ggml-base.en.bin`), the Silero VAD model and a photo library in `models/`.
Point `LS_ASSETS` at the asset library, or build a small local library with `./scripts/make_dev_library.sh`
and index it with `ls-index`.

```bash
./demo.sh airpods                                           # live from a mic (or: builtin)
LS_SOURCE=wav:fixtures/audio/luna-edit-talk.wav ./target/release/live-slides   # replay a recording
./target/release/ls-replay fixtures/audio/luna-edit-talk.wav                    # headless replay + summary
cargo run -p ls-agent --bin ls-agent-probe -- --runs 3      # 42 speech → board test cases against the model
```

Stage keys: `f` full screen · `b` blank · `g` grid of everything shown so far.
Useful settings: `LS_SOURCE`, `LS_THEME` (`sketch` or `slate`), `LS_FULLSCREEN`, `LS_DISPLAY`, `CANVAS_MODEL`,
`AGENT_RPM`. Names Whisper mangles ("Baseten", "Kafka") go in `talk-terms.txt`.

## More

- [TRIGGERS.md](TRIGGERS.md): what to say and how each decision is made
- [CANVAS.md](CANVAS.md): board model, ops and renderer design
- [app/DEMO_SCRIPT.md](app/DEMO_SCRIPT.md): a 3½-minute talk that exercises everything
- [probes/luna/README.md](probes/luna/README.md): the speech → board test suite
