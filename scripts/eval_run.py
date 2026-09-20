#!/usr/bin/env python3
"""Score a run log against expected (keyword, image_id) pairs.

usage: eval_run.py <logs/run-*.jsonl> <expected.tsv>
expected.tsv: keyword<TAB>image_id  (in talk order; image_id "-" = should NOT change the picture)

Reports: each render with the keyword it followed, correct/wrong, missed subjects, minimum gap
between renders (flicker), and keyword→render latency (from the first chunk whose text contains the
keyword being emitted by ASR, to the render; add up to one ASR tick ≈ 0.75 s for the true word time).
"""
import json, sys, statistics as st

log = [json.loads(l) for l in open(sys.argv[1])]
rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[2]) if l.strip() and not l.startswith("#")]
# "keyword<TAB>!image" rows are negatives: mentioned without display intent, must NOT be shown.
forbidden = {img[1:] for _, img in rows if img.startswith("!")}
expected = [(kw, img) for kw, img in rows if not img.startswith("!")]

chunks = [e for e in log if e.get("ev") == "chunk"]
renders = [e for e in log if e.get("ev") == "render"]
# First time each keyword appears in any chunk (curr or final), in wall ms.
first_seen = {}
for c in chunks:
    text = c["chunk"]["text"].lower()
    for kw, _ in expected:
        if kw.lower() in text and kw not in first_seen:
            first_seen[kw] = c["t_ms"]

rows, correct, wrong = [], 0, 0
for r in renders:
    got = r.get("image_id") or "(clear)"
    # Correct if a keyword mapped to this image was heard in the 3 s before the render.
    recent = [c for c in chunks if r["t_ms"] - 3000 <= c["t_ms"] <= r["t_ms"]]
    kws = [kw for kw, img in expected if img == got and any(kw.lower() in c["chunk"]["text"].lower() for c in recent)]
    kw = kws[0] if kws else None
    ok = kw is not None
    correct += ok
    wrong += (not ok)
    lat = None
    if kw:
        first = min(c["t_ms"] for c in recent if kw.lower() in c["chunk"]["text"].lower())
        lat = r["t_ms"] - first
    rows.append((r["t_ms"], kw, got if ok else "?", got, ok, lat))

shown = {r[3] for r in rows}
missed = [(kw, img) for kw, img in expected if img != "-" and img not in shown]
gaps = [b[0] - a[0] for a, b in zip(rows, rows[1:])]
lats = [r[5] for r in rows if r[4] and r[5] is not None]

for t, kw, want, got, ok, lat in rows:
    print(f"{t/1000:7.1f}s  {'OK ' if ok else 'BAD'}  after '{kw}' want {want} got {got}  keyword→render {lat} ms")
false_pos = [r[3] for r in rows if r[3] in forbidden]
print(f"\nrenders={len(rows)} correct={correct} wrong={wrong} missed={missed} false_positives={false_pos}")
if gaps:
    print(f"min gap between changes: {min(gaps)} ms")
if lats:
    print(f"keyword-in-transcript→render ms: p50={st.median(lats):.0f} max={max(lats)}  (true word time adds ≤ ~750 ms)")
summ = [e for e in log if e.get("ev") == "summary"]
if summ:
    s = summ[-1]
    sm = s["summary"]
    print("agent calls:", sm.get("agent_calls"), "fallbacks:", sm.get("agent_fallbacks"), "ops applied/refused:", sm.get("ops_applied"), sm.get("ops_refused"), "generated:", sm.get("generated"))
