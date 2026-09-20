# Luna probe set

Speech in, board out. Each case builds a board, gives the canvas agent (Luna) some speech, applies whatever
it returns to a real `Canvas`, and checks the **resulting board** — not which tools were called — so the same
cases keep working while the tool set and prompt change.

```
cargo run -p ls-agent --bin ls-agent-probe -- --dry                  # validate cases, print boards, no API calls
cargo run -p ls-agent --bin ls-agent-probe -- --runs 3               # real model; key from env or repo .env
cargo run -p ls-agent --bin ls-agent-probe -- --group chart-edit -v  # one group, verbose
cargo run -p ls-agent --bin ls-agent-probe -- --filter diagram-rename --model anthropic/claude-haiku-4.5
cargo run -p ls-agent --bin ls-agent-probe -- --ws-smoke             # two continued WebSocket turns
cargo run -p ls-agent --bin ls-agent-probe -- --ws-latency 30        # chained service-tier p50/p95
```

Flags: `--cases <path>` `--runs N` (default 3; models are not deterministic) `--filter <substr of id>`
`--group <name>` `--rpm N` `--verbose` `--ws-smoke` `--ws-latency N` `--model <id>` (else `CANVAS_MODEL`, else the default).
OpenAI probes use a fresh Responses WebSocket per independent case; `--ws-smoke` deliberately keeps one chain
for a chart creation followed by a correction. Set `CANVAS_TRANSPORT=http` to compare against Chat Completions. OpenAI
requests use the standard tier by default and verbose output shows the returned service tier.
Set `CANVAS_SERVICE_TIER=fast` to run the same workload through Fast mode (`priority` in the response).

**Rate limit:** a new OpenRouter account is capped at 20 requests/min per model; the runner paces at 18/min,
so 38 cases × 3 runs takes about 6 minutes. A 429 makes the agent fall back to offline rules; the runner
reports those runs as **errors**, never as pass or fail.

Output per case: `PASS` (all runs), `FLAKY` (some), `FAIL` (none), then the ops the model returned and what
was wrong. Exit code 1 if anything failed.

## Case format (`cases.json`, an array)

```json
{
  "id": "chart-correct-explicit",
  "group": "chart-edit",
  "board": [ {"op": {"op": "draw_chart", "kind": "bar", "title": "Users", "unit": null,
                     "points": [{"label": "Jan", "value": 40}]}} ],
  "history": ["earlier sentences, oldest first"],
  "newest": "the sentence being said now",
  "needs": "chart",
  "expect": { "elements": 1, "charts": {"e1": {"points": {"Jan": 45}}} },
  "pending": "why this case is skipped for now"
}
```

- `board`: setup steps applied in order. `{"photo": "eagle"}` adds a photo tile; `{"op": …}` is any
  `ls_canvas::Op` as JSON (`draw_chart`, `draw_diagram`, …). Ids are assigned in creation order: `e1`, `e2`, …
  (`--dry` prints each board so you can check).
- `history` / `newest`: today these become `previous_speech` / `newest_speech`; after refactor step 15 they
  feed `transcript` / `speaking_now`.
- `needs`: optional Jev-style hint (`chart` | `diagram` | `board`). Leave it out — the target design has no Jev.
- `pending`: skips the case (used for photo cases until the `show_photo` tool exists).

### `expect` keys (all optional, all must hold)

| key | meaning |
|---|---|
| `elements` | exact number of tiles |
| `present` / `absent` | element ids that must / must not be on the board |
| `cleared` | board has no tiles |
| `no_change` | tiles' content, layout and annotations identical to before (focus is ignored) |
| `layout` | `auto` \| `hero` \| `compare` \| `grid` |
| `focus` | id of the focused tile |
| `annotated` | ids that must be targeted by an annotation |
| `charts` | `{id: {points, values, kind, title_contains}}` — `points` is an exact label→value map (labels case-insensitive, count must match, so a chart wiped to one bar fails); `values` is an order-free list of values (use when labels may vary) |
| `diagrams` | `{id: {nodes_include, nodes_exclude, node_count, layout}}` — node matches are case-insensitive substrings |
| `photo`, `no_photo` | only for `pending` cases until `show_photo` exists |

Add new cases to the generator-free JSON directly; keep groups: `chart-edit`, `chart-guard`, `diagram-edit`,
`board`, `photo`.

## Groups added with the logo/icon change (2026-09-20)
`ambient` (17 cases): technologies, companies and concepts that are only *mentioned* should get a logo or icon, and
decoys must stay quiet (apple, "go over", swift, "cut me some slack", rust, filler, a foil, something already on the
board). Also `diagram-guard-no-placeholder-steps`: "our pipeline has four steps" must not draw "Step 1…Step 4".
Spoken checks: `fixtures/audio/ambient-symbols-talk.wav` + `scripts/e2e_symbols.py`.
