# Canvas mode — design (2026-09-19)

User decisions: **library images + annotations**, **evolving board** (≈1–4 elements that build up and
regroup), **fast gate + agent** (today's ~1 s path puts the image up; an agent refines layout 1–2 s later),
**model bake-off** (OpenRouter tool calling, model-agnostic; pick Claude Haiku 4.5 vs Gemini Flash-Lite on
real timings once the key exists). *The "fast gate + agent" split was replaced by one decision-maker on
2026-09-19 (Flow, below); the model is now `openai/gpt-5.6-luna`.*

## Flow (2026-09-19 refactor: Luna decides everything)
```
speech → Whisper (partial + final phrases) → transcript
          │ whenever Luna is idle, ≥ 3 new words, under the rate cap
          ▼
   Luna (OpenAI Responses WebSocket, or HTTP/OpenRouter chat) sees: transcript · board with ids · recent changes · newest words
          → tool calls: show_photo / draw_chart / draw_text / … / remove / clear_board / no_action
          ▼
   Rust: guards (quote for destructive ops, spoken numbers) → Canvas ops by id → Scene v+1 → emit "scene"
          show_photo → CLIP search of the library → (nothing?) image generation → Canvas render → "scene"
```
- The web view renders the whole Scene each time; elements animate between rects (CSS transitions),
  annotations are an SVG overlay. Rust owns all state.
- There is no separate "fast path": a photo is a `show_photo` request that Luna makes and Rust fulfils. The old
  Jev gate, the phrase model and the stage's hold/confirm rules were removed (see TRIGGERS.md).
- With `CANVAS_TRANSPORT=websocket`, startup prepares the fixed instructions and tools using `generate: false`.
  The first turn sends full context; later turns continue with `previous_response_id`, tool outcomes, new speech,
  and the authoritative current board. A failed socket is discarded and that turn retries over HTTP.
- OpenAI requests use the standard service tier on both transports because the chained Luna benchmark found it
  faster and more consistent. `CANVAS_SERVICE_TIER=fast` opts into Fast mode; agent logs record the returned tier.

## Scene
- `Element { id, kind, image_id, caption, rect (0..1), z, focus, diagram?, chart?, text? }` — ≤ 4 tiles (oldest evicted).
- `Annotation { id, kind: highlight|frame|arrow, targets: [element ids], label? }` — ≤ 3.
- `layout`: auto | hero | compare | grid — the layout engine turns (elements, layout, focus) into rects.
  auto: 1 → full; 2 → side by side; 3 → hero + 2; 4 → 2×2. hero: focus big, others stacked; compare: 2 up.

## Board operations (validated; unknown ids or labels are refused and logged)
- Photos: `render` adds a tile and focuses it (oldest evicted past 4), `update` replaces the focused photo in place
  (for `show_photo` mode `replace`); `clear` empties the board.
- Agent ops address tiles by id (`e1`), diagram nodes by id or label (`n2` / "Build"), chart points by label.
  Because they are id-addressed they apply even if the board changed while Luna was thinking; `clear_board`
  clears only the tiles Luna saw. `Canvas::take_notes()` returns why an op was refused.
Offline rules (no key): "compare/versus/side by side" → compare; "focus on/this one/zoom" → focus latest;
"notice/look at the/see how" → highlight latest; "let's move on/next topic/new section" → clear board.

## Contracts
`RenderEvent` stays (logs, eval, replay). New `Scene` is emitted alongside to the web view.

## Live diagrams, charts and text (2026-09-19)
- Tiles are `kind: image | diagram | chart | text | logo`. A `logo` tile is a company logo, an icon or a flag from the symbol
  library (`image_id` = its id, drawn as a plain image with its name under it), or a **name card** (`image_id` empty, just
  the name in handwriting) when the library has nothing. `Canvas::render_logo` dedupes on the asset id or the name.
  Photos come from `show_photo`; diagrams and charts from Luna's
  tools (full list and rules: TRIGGERS.md, "How each decision is made"). Charts are corrected with `set_point`
  (one value), grown with `add_point`, trimmed with `remove_point`; diagrams with `add_nodes`, `update_node`,
  `remove_node`, `add_edge`, `remove_edge`. There is no whole-data-set `update_chart` tool any more: a model that
  sent only the changed point used to wipe the rest of the chart.
- A text tile holds up to eight ordered semantic blocks: `heading`, `paragraph` and `bullet`. Blocks get stable ids
  (`b1`…) so Luna can append, correct or remove one block without redrawing the tile. `emphasis` is a list of exact
  phrases within the block; the renderer marks those phrases using the active theme. A closing such as “Thank you” is
  an ordinary heading block, rather than a special slide type. Text tools only apply after finished speech, so a partial
  ASR phrase cannot become visible copy.
- Canvas rules: omitted edges = chain (flow/timeline), chain + closing edge (cycle), spokes (hub); a redraw
  sharing ≥ half the nodes of a diagram on the board replaces it in place; same chart title → replace data;
  a stat with ≥ 2 values becomes bars; ≤ 8 nodes / points; node ids are never reused after a removal.
- Guards: chart values must be numbers actually spoken (or already on the board); destructive ops need a `quote`
  found in the newest words.
- Renderer: `app/dist/sketch.js` (both themes), SVG per tile, sized to the tile's final px; only new nodes / edges / bars /
  points animate (per-tile `seen` set). Browser preview without Tauri: serve `app/dist`, call `__scene(scene)`.
- Real-model replay (fixtures/audio/graphics-talk.wav, Haiku 4.5): users 2K → 15K → 40K as bars, pie
  60/30/10, a 4-step flow that became a cycle on "it all runs in a loop"; graphics land 1.1–2.7 s after the
  sentence. canvas-talk unchanged: 6/6, 0 false positives, p50 1.1 s.

## "Live sketch" theme (2026-09-19, default; `LS_THEME=slate` restores the dark cards)
- Warm paper with fibre grain; photos are taped polaroids (tilt + tape angle seeded by element id) with a
  handwritten caption; diagrams/charts are drawn straight onto the page.
- `app/dist/sketch.js` (`render(svg, el, W, H, seen, full, theme)`; the old separate `graphics.js` is gone, `slate` is now a skin of this renderer): hand-drawn primitives —
  bowed strokes that overshoot their ends, loose ellipses that overlap where they started, clipped hatching,
  two-stroke arrowheads; colour washes deliberately offset from outlines. Seeded PRNG per element/node, so a
  graphic never re-wobbles on re-render. Strokes draw themselves (dash offset); handwriting writes left→right.
- Fonts are macOS built-ins (Noteworthy → Chalkboard SE → Marker Felt), no downloads.
- Highlights are red marker circles around the tile with a handwritten label; arrows are sketched curves.
- Preview without the app: serve the repo root, open /app/dist/index.html (loads preview.js), call
  `demo.photos() / demo.charts() / demo.diagrams() / demo.board() / demo.full('pie')`; `?theme=slate` for the old look.


## Routing
There is none: Luna is called on the newest words and chooses the tool. (Until 2026-09-19 Jev routed each
sentence to `photo | photo_update | chart | diagram | board | clear | none`; it had no route for "edit what is on
screen", so value corrections never reached the agent — 0 of 9 phrasings, see `probes/luna/BASELINE.md`.)


## Pictures and text (2026-09-20)
- `Node.icon` and `Point.icon` hold an image url (`img://localhost/<id>.svg`) once the pipeline has resolved Luna's `logo` / `icon` hints
  (`NodeSpec.logo`, `Point.logo` are hints only and are never sent to the web view). `MAX_NODES` is 10.
- The web view draws them as `<image>` inside the node, above the label; in a bar/line chart under the axis, above the label; in a pie legend
  next to the name. The app serves `.svg` with `image/svg+xml` through the shared `ImageCache::mime_of` (the app once had its own copy that
  labelled SVGs `image/jpeg`, which showed as a broken-image icon).
- Text fitting, the technical-diagram layout and the icon rules: see TRIGGERS.md, "Diagrams and charts".
