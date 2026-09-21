# Agent probe set

Speech in, board out. Each case builds a board, gives the agent (Luna) some speech, applies whatever it returns to a
real `Canvas`, and checks the resulting board, not which tools were called. The cases keep working while the tools
and prompt change.

```
cargo run -p ls-agent --bin ls-agent-probe -- --dry                  # validate cases and print boards, no API calls
cargo run -p ls-agent --bin ls-agent-probe -- --runs 3               # real model, key from env or repo .env
cargo run -p ls-agent --bin ls-agent-probe -- --group chart-edit -v  # one group, verbose
cargo run -p ls-agent --bin ls-agent-probe -- --filter diagram-rename --model <model id>
cargo run -p ls-agent --bin ls-agent-probe -- --ws-smoke             # two chained WebSocket turns
```

Flags: `--cases <path>`, `--runs N` (default 3, since models are not deterministic), `--filter <substring of id>`,
`--group <name>`, `--rpm N`, `--verbose`, `--model <id>` (else `CANVAS_MODEL`, else the default).

- OpenAI probes use a fresh Responses WebSocket per case. `--ws-smoke` keeps one chain across a chart creation and
  a correction. Set `CANVAS_TRANSPORT=websocket` to exercise that path.
- OpenRouter accounts have low request limits, so the runner paces itself. A rate-limited call is reported as an
  error, never as a pass or fail.
- Output per case is `PASS` (all runs), `FLAKY` (some) or `FAIL` (none), then the ops returned and what was wrong.
  The exit code is 1 if anything failed.

## Case format

`cases.json` is an array of cases:

```json
{
  "id": "chart-correct-explicit",
  "group": "chart-edit",
  "board": [ {"op": {"op": "draw_chart", "kind": "bar", "title": "Users", "unit": null,
                     "points": [{"label": "Jan", "value": 40}]}} ],
  "history": ["earlier sentences, oldest first"],
  "newest": "the sentence being said now",
  "expect": { "elements": 1, "charts": {"e1": {"points": {"Jan": 45}}} }
}
```

- `board`: setup steps applied in order. `{"photo": "eagle"}` adds a photo tile and `{"op": ...}` is any
  `ls_canvas::Op` as JSON. Ids are assigned in creation order (`e1`, `e2`, ...). Use `--dry` to see each board.
- `history` and `newest`: the earlier transcript and the words being said now.
- `pending`: optional reason to skip a case.

### `expect` keys

All are optional and all must hold.

| Key | Meaning |
|---|---|
| `elements` | Exact number of tiles |
| `present`, `absent` | Element ids that must or must not be on the board |
| `cleared` | The board has no tiles |
| `no_change` | Tile content, layout and annotations are identical to before (focus is ignored) |
| `layout` | `auto`, `hero`, `compare` or `grid` |
| `focus` | Id of the focused tile |
| `annotated` | Ids that must be targeted by an annotation |
| `charts` | `{id: {points, values, kind, title_contains}}`. `points` is an exact label to value map (labels are case-insensitive and the count must match, so a chart wiped to one bar fails). `values` is an order-free list of values. |
| `diagrams` | `{id: {nodes_include, nodes_exclude, node_count, layout}}`. Node matches are case-insensitive substrings. |

Groups: `board`, `chart-edit`, `chart-guard`, `chart-headline`, `diagram-edit`, `tech-diagram`, `text`, `symbol`, `photo` and `ambient`.

`ambient` checks that technologies, companies and concepts that are only mentioned get a logo or icon, and that
decoys stay quiet (apple, "go over", swift, "cut me some slack", rust, filler, a comparison, something already on the
board).
