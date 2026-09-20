#!/usr/bin/env python3
"""End-to-end check of a run log against the milestones of fixtures/audio/luna-edit-talk.wav.

usage: e2e_check.py <logs/run-*.jsonl> [--any-order]

Reads the `scene` events (the board after every change) and checks the board went through these states IN
ORDER (a later milestone must come after the earlier one). Whisper hears real synthetic speech, so this covers
ASR → Luna → guards → canvas, the whole chain, not one link.
"""
import json, re, sys

log = [json.loads(l) for l in open(sys.argv[1])]
scenes = [e for e in log if e.get("ev") == "scene"]

def charts(s):
    return [t for t in s["tiles"] if t["kind"] == "chart"]

def values(t):
    return sorted(round(p[1], 6) for p in t["points"])

def has_chart(vals):
    return lambda s: any(values(t) == sorted(vals) for t in charts(s))

def images(s):
    return [t for t in s["tiles"] if t["kind"] == "image"]

def diagram_labels(s):
    return [" ".join(t["nodes"]).lower() for t in s["tiles"] if t["kind"] == "diagram"]

MILESTONES = [
    ("bar chart of 40, 55, 70", has_chart([40, 55, 70])),
    ("March corrected: 40, 55, 80", has_chart([40, 55, 80])),
    ("April added: 40, 55, 80, 95", has_chart([40, 55, 80, 95])),
    ("a photo is on the board (eagle)", lambda s: len(images(s)) >= 1),
    ("photo taken away, chart still there", lambda s: len(images(s)) == 0 and any(len(t["points"]) == 4 for t in charts(s))),
    ("pipeline diagram (commit, build, test, deploy)", lambda s: any(all(w in d for w in ("commit", "build", "test", "deploy")) for d in diagram_labels(s))),
    ("test step removed from the diagram", lambda s: any("commit" in d and "deploy" in d and "test" not in d for d in diagram_labels(s))),
    ("board cleared", lambda s: s["n"] == 0),
    ("a photo is on the board (owl)", lambda s: len(images(s)) >= 1),
]

i, reached = 0, []
if "--any-order" in sys.argv:
    # each milestone on its own: did the board EVER look like this (an early miss no longer hides later hits)
    for name, pred in MILESTONES:
        hit = next((s["version"] for s in scenes if pred(s)), None)
        if hit is not None:
            reached.append((name, hit))
else:
    for s in scenes:
        while i < len(MILESTONES) and MILESTONES[i][1](s):
            reached.append((MILESTONES[i][0], s["version"]))
            i += 1
            if i >= len(MILESTONES):
                break
for n, (name, _) in enumerate(MILESTONES):
    hit = next((v for m, v in reached if m == name), None)
    print(f"{'OK  ' if hit is not None else 'MISS'} {n + 1}. {name}" + (f"  (scene v{hit})" if hit is not None else ""))
print(f"\n{len(reached)}/{len(MILESTONES)} milestones" + ("" if "--any-order" in sys.argv else ", in order"))

# what the model was asked and did, for the misses
calls = [e for e in log if e.get("ev") == "agent"]
print(f"agent calls={len(calls)}  fallbacks={sum(1 for c in calls if c.get('source') == 'Rules')}  "
      f"refused ops={sum(len(c.get('refused', [])) for c in calls)}")
lat = sorted(c["ms"] for c in calls)
if lat:
    print(f"agent latency p50={lat[len(lat)//2]} ms max={lat[-1]} ms")
rend = [e["speech_to_render_ms"] for e in log if e.get("ev") == "render"]
if rend:
    print(f"photo speech→render ms: {rend}")
sys.exit(0 if len(reached) == len(MILESTONES) else 1)
