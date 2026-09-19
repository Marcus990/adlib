# Canvas mode — design (2026-09-19)

User decisions: **library images + annotations**, **evolving board** (≈1–4 elements that build up and
regroup), **fast gate + agent** (today's ~1 s path puts the image up; an agent refines layout 1–2 s later),
**model bake-off** (OpenRouter tool calling, model-agnostic; pick Claude Haiku 4.5 vs Gemini Flash-Lite on
real timings once the key exists).

## Flow
```
speech → Jev display-intent gate ‖ query → search → stage (hold/pending/newest-wins)   ← unchanged
          │ Rendered(render|update|clear)
          ▼
   Canvas (Rust, crates/canvas): fast op → Scene v+1 → emit "scene"      (≈ today's latency)
          │ (debounced ~300 ms, only if the board changed)
          ▼
   Canvas agent (OpenRouter chat + tools; offline rule fallback) sees Scene + recent transcript
          → tool calls (focus / remove / arrange / annotate / clear_annotations)
          → validated ops applied iff scene.version unchanged → Scene v+2 → emit "scene"
```
- The fast path never waits for the agent; a stale agent answer (scene changed meanwhile) is dropped.
- The web view renders the whole Scene each time; elements animate between rects (CSS transitions),
  annotations are an SVG overlay. Rust owns all state.

## Scene
- `Element { id, image_id, caption, rect (0..1), z, focus }` — ≤ 4 images (oldest evicted).
- `Annotation { id, kind: highlight|frame|arrow, targets: [element ids], label? }` — ≤ 3.
- `layout`: auto | hero | compare | grid — the layout engine turns (elements, layout, focus) into rects.
  auto: 1 → full; 2 → side by side; 3 → hero + 2; 4 → 2×2. hero: focus big, others stacked; compare: 2 up.

## Fast-path mapping (deterministic, no LLM)
- render → add image, focus it, layout auto (evict oldest past 4)
- update → replace the focused image in place (keeps position)
- clear → empty the board (and annotations)

## Agent tools (validated; unknown ids ignored)
- `focus(id)`, `remove(id)`, `arrange(layout)`, `annotate(kind, targets, label?)`, `clear_annotations()`
Offline rules (no key): "compare/versus/side by side" → compare; "focus on/this one/zoom" → focus latest;
"notice/look at the/see how" → highlight latest; "let's move on/next topic/new section" → clear board.

## Contracts
`RenderEvent` stays (logs, eval, replay). New `Scene` is emitted alongside to the web view.

## Live diagrams and charts (2026-09-19, user choice: "live diagrams" + "charts from speech")
- Tiles are `kind: image | diagram | chart` (`Element.diagram` / `Element.chart`). Images still come from the
  fast path; diagrams and charts only from the agent.
- Agent tools (10 total): `draw_diagram(layout: flow|cycle|hub|timeline, title?, nodes[{label, icon?, note?}],
  edges?[{from, to, label?}])`, `extend_diagram(id, nodes, edges?)`, `draw_chart(kind: bar|line|pie|stat, title?,
  unit?, points[{label, value}])`, `update_chart(id, kind?, title?, points)` (full data set, replaces).
- Canvas rules: omitted edges = chain (flow/timeline), chain + closing edge (cycle), spokes (hub); a redraw
  sharing ≥ half the nodes of a diagram on the board replaces it in place; same chart title → replace data;
  a stat with ≥ 3 values becomes bars; ≤ 8 nodes / points; additive ops apply even if the board changed
  while the agent was thinking (layout ops still need the version to match).
- Triggers: `has_graphic_cue` (digits, number words, "first/then/finally", "process", "cycle", "grew"…) in new
  words → agent call when the sentence completes (final chunk or Whisper closes it with . ? !). The agent sees
  the last 5 finished phrases + the newest speech + the board (incl. node labels / chart points).
- Code-level guards (prompt rules weren't reliable): chart values must be numbers actually spoken (digits or
  words, `spoken_numbers`) or already on the board — drops invented remainders; `clear_board` only when the
  newest words close a section (`has_section_cue`).
- Renderer: `app/dist/graphics.js`, SVG per tile, sized to the tile's final px; only new nodes / edges / bars /
  points animate (per-tile `seen` set). Browser preview without Tauri: serve `app/dist`, call `__scene(scene)`.
- Real-model replay (fixtures/audio/graphics-talk.wav, Haiku 4.5): users 2K → 15K → 40K as bars, pie
  60/30/10, a 4-step flow that became a cycle on "it all runs in a loop"; graphics land 1.1–2.7 s after the
  sentence. canvas-talk unchanged: 6/6, 0 false positives, p50 1.1 s.

## "Live sketch" theme (2026-09-19, default; `LS_THEME=slate` restores the dark cards)
- Warm paper with fibre grain; photos are taped polaroids (tilt + tape angle seeded by element id) with a
  handwritten caption; diagrams/charts are drawn straight onto the page.
- `app/dist/sketch.js` (same `render(svg, el, W, H, seen, full)` API as graphics.js): hand-drawn primitives —
  bowed strokes that overshoot their ends, loose ellipses that overlap where they started, clipped hatching,
  two-stroke arrowheads; colour washes deliberately offset from outlines. Seeded PRNG per element/node, so a
  graphic never re-wobbles on re-render. Strokes draw themselves (dash offset); handwriting writes left→right.
- Fonts are macOS built-ins (Noteworthy → Chalkboard SE → Marker Felt), no downloads.
- Highlights are red marker circles around the tile with a handwritten label; arrows are sketched curves.
- Preview without the app: serve the repo root, open /app/dist/index.html (loads preview.js), call
  `demo.photos() / demo.charts() / demo.diagrams() / demo.board() / demo.full('pie')`; `?theme=slate` for the old look.


## Routing (2026-09-19)
Jev decides the kind of visual, not keyword lists: `photo | photo_update | chart | diagram | board | clear | none`
(one choice, alongside the intent probability, in the same ~212 ms call). The agent is called only when Jev asks
for chart/diagram/board, and it receives `needs: "chart"|"diagram"|"board"` in its user message. The photo path
runs only on `photo`/`photo_update`. Keyword triggers remain for the offline path.
