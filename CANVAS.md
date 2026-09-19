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
