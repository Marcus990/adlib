#!/usr/bin/env python3
"""End-to-end check of a run log against the milestones of fixtures/audio/luna-edit-talk.wav.

usage: e2e_check.py <logs/run-*.jsonl> [--any-order] [--talk edit|logo|tech]

  --talk edit (default): fixtures/audio/luna-edit-talk.wav, charts / diagrams / photos / clear.
  --talk logo: fixtures/audio/luna-logo-talk.wav, logos / icons / flags / name cards.
  --talk tech: fixtures/audio/luna-tech-talk.wav, an architecture diagram with logos and icons, then a chart of logos.

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

def logos(s):
    return [t for t in s["tiles"] if t["kind"] == "logo"]

def has_logo(asset=None, card=None):
    """A logo tile with this library id, or a name card (no asset) with this name."""
    def f(s):
        return any((asset and t["asset"] == asset) or (card and not t["asset"] and t["title"].lower() == card.lower()) for t in logos(s))
    return f

LOGO_MILESTONES = [
    ("Google logo (from the symbol library, not a generated picture)", has_logo("logos:google-icon")),
    ("Microsoft logo", has_logo("logos:microsoft-icon")),
    ("teamwork icon (a library icon)", lambda s: any(t["asset"].startswith(("lucide:", "simple-icons:")) for t in logos(s))),
    ("flag of Canada", has_logo("circle-flags:ca")),
    ("Hooli: not in the library, so a plain name card", has_logo(card="Hooli")),
    ("board cleared", lambda s: s["n"] == 0),
]

def diagram_tiles(s):
    return [t for t in s["tiles"] if t["kind"] == "diagram"]

def arch(s):
    for t in diagram_tiles(s):
        names = " ".join(t["nodes"]).lower()
        if all(w in names for w in ("gateway", "auth", "order", "postgres", "redis", "kafka")):
            yield t

TECH_MILESTONES = [
    ("architecture diagram with all six components", lambda s: any(True for _ in arch(s))),
    ("... and at least 6 edges (fan-out drawn, not a chain)", lambda s: any(t["edges"] >= 6 for t in arch(s))),
    ("... and pictures on at least 4 nodes (logos and icons resolved)", lambda s: any(t.get("pictures", 0) >= 4 for t in arch(s))),
    ("cloud market chart with 3 points", lambda s: any(t["kind"] == "chart" and len(t["points"]) == 3 for t in s["tiles"])),
    ("... with a logo on each point", lambda s: any(t["kind"] == "chart" and len(t["points"]) == 3 and t.get("pictures", 0) == 3 for t in s["tiles"])),
]

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

if "--talk" in sys.argv:
    MILESTONES = {"logo": LOGO_MILESTONES, "tech": TECH_MILESTONES}.get(sys.argv[sys.argv.index("--talk") + 1], MILESTONES)
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
    print(f"speech→render ms: {rend}")
for e in log:
    if e.get("ev") == "symbol":
        print(f"  symbol {e['tool']}({e['queries'][0]!r}{' +' + str(len(e['queries']) - 1) + ' synonyms' if len(e['queries']) > 1 else ''}) -> {(e['pick'] or {}).get('id', 'NAME CARD')}")
sys.exit(0 if len(reached) == len(MILESTONES) else 1)
