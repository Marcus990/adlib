#!/usr/bin/env python3
"""Render scenes in headless Chrome using the REAL web view (app/dist/index.html), for looking at diagrams and charts.

usage: preview_scene.py <cases.py | cases.json> <out-dir> [--theme sketch|slate|both] [--size 1600x900] [--only name,name]

A cases file (JSON list, or a .py file defining CASES) holds {"name": ..., "scene": {...}} entries; a scene is what the
app sends to the web view (elements with kind/diagram/chart/rect/focus, layout, annotations). Tiles without a rect get
the whole screen. Icon urls may be file:// paths to the card's SVGs (the browser preview has no img:// protocol).
Output: <out-dir>/<name>.<theme>.png, and a line per render saying whether any text left its tile or box, overlapped
another text, ran into a node, or had a word split between letters. Exit status 1 if any render has a problem, so it
can gate a change to the renderer:  python3 scripts/preview_scene.py scripts/preview/text_cases.py /tmp/out --theme both
"""
import json, os, re, shutil, subprocess, sys, tempfile, runpy
import html as html_lib
from pathlib import Path

CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
DIST = Path(__file__).resolve().parent.parent / "app" / "dist"

# Runs inside the page after the scene is drawn: every <text> must sit inside its tile and inside its own box, and no
# two texts may overlap. Findings go into <pre id="__overflow"> as JSON for the caller.
CHECK_JS = r"""
setTimeout(() => {
  const out = [], tiles = [...document.querySelectorAll('#board .tile')];
  const label = t => (t.textContent || '').trim().slice(0, 28);
  for (const tile of tiles) {
    const svg = tile.querySelector('svg.g'); if (!svg) continue;
    const tr = tile.getBoundingClientRect();
    const texts = [...svg.querySelectorAll('text')].filter(t => (t.textContent || '').trim());
    const boxes = texts.map(t => ({ t, r: t.getBoundingClientRect() }));
    for (const { t } of boxes) if (t.dataset.broke) out.push(`"${label(t)}" has a word split between letters`);
    for (const { t, r } of boxes) {
      const pad = 1.5;
      if (r.left < tr.left - pad || r.right > tr.right + pad || r.top < tr.top - pad || r.bottom > tr.bottom + pad)
        out.push(`"${label(t)}" leaves its tile`);
      const box = t.closest('[data-box]');
      if (box) {
        const [x, y, w, h] = box.dataset.box.split(',').map(Number), s = svg.getBoundingClientRect(), vb = svg.viewBox.baseVal, k = s.width / vb.width;
        const bx = s.left + x * k, by = s.top + y * k, bw = w * k, bh = h * k;
        if (r.left < bx - pad || r.right > bx + bw + pad || r.top < by - pad || r.bottom > by + bh + pad) out.push(`"${label(t)}" sticks out of its box`);
      }
    }
    // a label that is not a node's own text must not run into any node
    const sr = svg.getBoundingClientRect(), vbw = svg.viewBox.baseVal.width, kk = sr.width / vbw;
    const nodeRects = [...svg.querySelectorAll('[data-box]')].map(b => { const [x, y, w, h] = b.dataset.box.split(',').map(Number); return { b, l: sr.left + x * kk, t: sr.top + y * kk, r: sr.left + (x + w) * kk, bt: sr.top + (y + h) * kk }; });
    for (const { t, r } of boxes) {
      if (t.closest('[data-box]')) continue;
      for (const n of nodeRects) {
        const ox = Math.min(r.right, n.r) - Math.max(r.left, n.l), oy = Math.min(r.bottom, n.bt) - Math.max(r.top, n.t);
        if (ox > 3 && oy > 3) out.push(`"${label(t)}" runs into a node`);
      }
    }
    for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i].r, b = boxes[j].r;
      const ox = Math.min(a.right, b.right) - Math.max(a.left, b.left), oy = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
      if (ox > 2 && oy > 2 && ox * oy > 0.12 * Math.min(a.width * a.height, b.width * b.height)) out.push(`"${label(boxes[i].t)}" overlaps "${label(boxes[j].t)}"`);
    }
  }
  const pre = document.createElement('pre'); pre.id = '__overflow'; pre.textContent = JSON.stringify(out); document.body.appendChild(pre);
}, 6500);
"""

def load(path):
    if path.endswith(".py"):
        return runpy.run_path(path)["CASES"]
    return json.load(open(path))

def render(case, theme, size, out_dir, tmp):
    view = Path(tmp) / f"{case['name']}-{theme}"
    shutil.copytree(DIST, view)
    sc = case["scene"]
    for e in sc["elements"]:
        e.setdefault("rect", {"x": 0, "y": 0, "w": 1, "h": 1})
        e.setdefault("z", 1); e.setdefault("focus", False); e.setdefault("url", ""); e.setdefault("image_id", ""); e.setdefault("caption", "")
        e.setdefault("diagram", None); e.setdefault("chart", None)
    sc.setdefault("version", 1); sc.setdefault("layout", "auto"); sc.setdefault("annotations", []); sc.setdefault("reason", "preview"); sc.setdefault("chunk_id", 0)
    html = (view / "index.html").read_text()
    html = html.replace("</body>", "<script>setTimeout(()=>window.__scene(%s),300)</script></body>" % json.dumps(sc), 1)
    (view / "index.html").write_text(html)
    (view / "overflow_check.js").write_text(CHECK_JS)
    html = (view / "index.html").read_text().replace("</body>", '<script src="overflow_check.js"></script></body>', 1)
    (view / "index.html").write_text(html)
    out = Path(out_dir) / f"{case['name']}.{theme}.png"
    w, h = size.split("x")
    subprocess.run([CHROME, "--headless=new", "--disable-gpu", "--allow-file-access-from-files", f"--window-size={w},{h}",
                    "--virtual-time-budget=9000", f"--screenshot={out}", f"file://{view}/index.html?theme={theme}"],
                   capture_output=True, text=True)
    # the same page again, dumping the DOM: overflow_check.js leaves its findings in #__overflow
    dom = subprocess.run([CHROME, "--headless=new", "--disable-gpu", "--allow-file-access-from-files", f"--window-size={w},{h}",
                          "--virtual-time-budget=9000", "--dump-dom", f"file://{view}/index.html?theme={theme}"], capture_output=True, text=True).stdout
    m = re.search(r'<pre id="__overflow"[^>]*>(.*?)</pre>', dom, re.S)
    problems = json.loads(html_lib.unescape(m.group(1))) if m else ["(no report)"]
    return out, problems

if __name__ == "__main__":
    a = sys.argv[1:]
    cases, out_dir = load(a[0]), a[1]
    theme = a[a.index("--theme") + 1] if "--theme" in a else "sketch"
    size = a[a.index("--size") + 1] if "--size" in a else "1600x900"
    only = a[a.index("--only") + 1].split(",") if "--only" in a else None
    os.makedirs(out_dir, exist_ok=True)
    failed = False
    with tempfile.TemporaryDirectory() as tmp:
        for c in cases:
            if only and c["name"] not in only:
                continue
            for th in (["sketch", "slate"] if theme == "both" else [theme]):
                out, problems = render(json.loads(json.dumps(c)), th, size, out_dir, tmp)
                failed = failed or bool(problems)
                print(f"{out.name}: " + ("OK, no text outside its box" if not problems else f"{len(problems)} PROBLEMS"))
                for p in problems[:14]:
                    print("    ", p)
    sys.exit(1 if failed else 0)
