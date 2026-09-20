// The diagram and chart renderer: Sketch.render(svg, element, W, H, seen, full, theme). One renderer, two skins.
// "sketch" (the default): drawn by hand while the presenter talks — marker strokes that draw themselves, handwritten
// labels, hatched fills on warm paper. "slate": the same layouts with clean lines, solid fills and a dark palette.
// Every string is MEASURED (textfit.js) and fitted into its box or bubble; nodes and chart points can carry a logo or
// icon picture. Randomness is seeded per element/node so a graphic never re-wobbles when the board re-renders; only
// new parts animate in.
(function () {
  const NS = 'http://www.w3.org/2000/svg';
  // Two skins of one renderer: "sketch" (hand-drawn, warm paper) and "slate" (clean lines, solid fills, dark cards).
  // They share every layout, text-fitting and icon rule; only these values differ. Set per render by applyTheme().
  const THEMES = {
    sketch: { INK: '#2b2723', INK_SOFT: '#6f665b', PAPER: '#f4ecdc', HAND: '"Noteworthy", "Chalkboard SE", "Marker Felt", cursive', ROUGH: 1, SOLID: false,
      MARK: ['#d1495b', '#2e86ab', '#e59b2f', '#3c9d5d', '#7b5ea7', '#d45d9b', '#1f7a8c', '#a0673a'] },
    slate: { INK: '#e5e9f0', INK_SOFT: '#94a3b8', PAPER: '#0d121c', HAND: '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif', ROUGH: 0, SOLID: true,
      MARK: ['#f87171', '#60a5fa', '#fbbf24', '#34d399', '#a78bfa', '#f472b6', '#5eead4', '#fb923c'] },
  };
  let INK, INK_SOFT, PAPER, HAND, ROUGH, SOLID, MARK;
  function applyTheme(name) { ({ INK, INK_SOFT, PAPER, HAND, ROUGH, SOLID, MARK } = THEMES[name] || THEMES.sketch); }
  applyTheme('sketch');

  // ---------- basics ----------
  function S(tag, attrs, parent) {
    const el = document.createElementNS(NS, tag);
    for (const k in attrs) if (attrs[k] !== undefined && attrs[k] !== null) el.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(el);
    return el;
  }
  function rng(key) {                                   // mulberry32 seeded by a string
    let h = 2166136261;
    for (let i = 0; i < key.length; i++) h = Math.imul(h ^ key.charCodeAt(i), 16777619);
    return () => {
      h = (h + 0x6d2b79f5) | 0;
      let t = Math.imul(h ^ (h >>> 15), 1 | h);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  const J = (r, a) => (r() - 0.5) * 2 * a * ROUGH;
  const f1 = v => v.toFixed(1);

  function anim(el, frames, delay, dur, easing) {
    try { return el.animate(frames, { duration: dur || 600, delay: delay || 0, easing: easing || 'cubic-bezier(.3,.7,.3,1)', fill: 'both' }); } catch (_) {}
  }
  // Strokes draw themselves like a pen moving (multi-subpath paths draw one stroke after another).
  function drawIn(path, delay, dur) {
    let len = 0;
    try { len = path.getTotalLength(); } catch (_) {}
    if (!len) return;
    path.style.strokeDasharray = `${len} ${len}`;
    anim(path, [{ strokeDashoffset: len }, { strokeDashoffset: 0 }], delay, dur || Math.min(900, 250 + len * 0.6), 'cubic-bezier(.45,.05,.55,.95)');
  }
  // Handwriting appears left to right.
  function writeIn(el, delay, dur) {
    const a = anim(el, [{ clipPath: 'inset(-20% 100% -20% 0)' }, { clipPath: 'inset(-20% 0% -20% 0)' }], delay, dur || 450, 'linear');
    if (!a) anim(el, [{ opacity: 0 }, { opacity: 1 }], delay, dur || 450);
  }

  // ---------- rough geometry (path data) ----------
  // One slightly bowed stroke that overshoots its ends a little, like a quick marker line.
  function lineD(r, x1, y1, x2, y2, rough) {
    rough *= ROUGH;
    const len = Math.hypot(x2 - x1, y2 - y1) || 1;
    const ux = (x2 - x1) / len, uy = (y2 - y1) / len;
    const a = Math.min(rough, 0.6 + len * 0.012);
    const ov = Math.min(len * 0.035, rough * 1.6);
    const sx = x1 - ux * ov * r() + J(r, a), sy = y1 - uy * ov * r() + J(r, a);
    const ex = x2 + ux * ov * r() + J(r, a), ey = y2 + uy * ov * r() + J(r, a);
    const bow = J(r, Math.min(len * 0.025, rough * 2.2));
    const mx = (sx + ex) / 2 - uy * bow, my = (sy + ey) / 2 + ux * bow;
    return `M${f1(sx)},${f1(sy)} Q${f1(mx)},${f1(my)} ${f1(ex)},${f1(ey)}`;
  }
  function rectD(r, x, y, w, h, rough) {
    return [lineD(r, x, y, x + w, y, rough), lineD(r, x + w, y, x + w, y + h, rough),
            lineD(r, x + w, y + h, x, y + h, rough), lineD(r, x, y + h, x, y, rough)].join(' ');
  }
  function smooth(pts) {                                 // Catmull-Rom → cubic Bézier
    let d = `M${f1(pts[0][0])},${f1(pts[0][1])}`;
    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)];
      d += ` C${f1(p1[0] + (p2[0] - p0[0]) / 6)},${f1(p1[1] + (p2[1] - p0[1]) / 6)} ${f1(p2[0] - (p3[0] - p1[0]) / 6)},${f1(p2[1] - (p3[1] - p1[1]) / 6)} ${f1(p2[0])},${f1(p2[1])}`;
    }
    return d;
  }
  // A loop drawn in one go: not quite round, and it overlaps where it started.
  function ellipseD(r, cx, cy, rx, ry, wobble) {
    const n = 20, start = r() * Math.PI * 2, over = (0.12 + r() * 0.14) * ROUGH, pts = [];
    for (let i = 0; i <= n; i++) {
      const t = start + (Math.PI * 2 * (1 + over) * i) / n, k = 1 + J(r, wobble || 0.04) + (i / n) * 0.03 * ROUGH;
      pts.push([cx + rx * k * Math.cos(t), cy + ry * k * Math.sin(t)]);
    }
    return smooth(pts);
  }
  // Hatching lines across a box (clip them to the real shape with a clipPath).
  function hatchD(r, x, y, w, h, gap, rough) {
    const ang = -0.72, dx = Math.cos(ang), dy = Math.sin(ang), span = w + h;
    let d = '';
    for (let s = -span; s < span; s += gap) {
      const cx = x + w / 2 + (-dy) * s, cy = y + h / 2 + dx * s;
      d += ' ' + lineD(r, cx - dx * span, cy - dy * span, cx + dx * span, cy + dy * span, rough * 0.6);
    }
    return d;
  }
  function arrowHeadD(r, x, y, ang, size) {
    const a1 = ang + Math.PI - 0.45 + J(r, 0.08), a2 = ang + Math.PI + 0.45 + J(r, 0.08);
    return lineD(r, x, y, x + Math.cos(a1) * size, y + Math.sin(a1) * size, 0.6) + ' ' +
           lineD(r, x, y, x + Math.cos(a2) * size, y + Math.sin(a2) * size, 0.6);
  }

  // ---------- drawing helpers ----------
  const stroke = (color, w, extra) => Object.assign({ fill: 'none', stroke: color, 'stroke-width': w, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, extra || {});
  function pen(g, d, color, w, isNew, delay, dur, extra) {
    const p = S('path', Object.assign({ d }, stroke(color, w, extra)), g);
    if (isNew) drawIn(p, delay, dur);
    return p;
  }
  // ---------- text: measured, never guessed (see textfit.js) ----------
  const fit = (text, maxW, maxH, size, min, o) => TextFit.fit(text, maxW, maxH, Object.assign({ size, min, family: HAND, maxLines: 2 }, o));
  const tw = (text, size, weight) => TextFit.width(String(text), size, weight || 400, HAND);
  const overlap = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  const isUrl = s => /^(img|file|https?):/.test(s || '');

  // `halo` paints a paper-coloured outline under the letters so a label stays readable where it crosses a line.
  function write(g, lines, x, y, size, attrs, isNew, delay) {
    const { halo, lh: lhIn, broke, ...rest } = attrs || {};
    if (broke) rest['data-broke'] = '1';
    const a = Object.assign({ x, y, 'font-size': size, fill: INK, 'text-anchor': 'middle', 'dominant-baseline': 'middle', 'font-family': HAND }, rest);
    if (halo) Object.assign(a, { 'paint-order': 'stroke', stroke: PAPER, 'stroke-width': size * 0.34, 'stroke-linejoin': 'round' });
    const t = S('text', a, g);
    const lh = lhIn || size * 1.12;
    (Array.isArray(lines) ? lines : [lines]).forEach((l, i, arr) => { S('tspan', { x, dy: i === 0 ? -((arr.length - 1) * lh) / 2 : lh }, t).textContent = l; });
    if (isNew) writeIn(t, delay, 280 + String(lines).length * 18);
    return t;
  }
  // A logo / icon picture (an <image> of the SVG the app serves), centred on cx,cy.
  function picture(g, url, cx, cy, size, isNew, delay) {
    const im = S('image', { href: url, x: cx - size / 2, y: cy - size / 2, width: size, height: size, preserveAspectRatio: 'xMidYMid meet' }, g);
    if (isNew) anim(im, [{ opacity: 0 }, { opacity: 1 }], delay, 400);
    return im;
  }
  function fmt(v, unit) {
    const u = (unit || '').trim(), a = Math.abs(v), trim = x => x.toFixed(1).replace(/\.0$/, '');
    let s;
    if (u === '%') s = trim(v) + '%';
    else if (a >= 1e9) s = trim(v / 1e9) + 'B';
    else if (a >= 1e6) s = trim(v / 1e6) + 'M';
    else if (a >= 1e3) s = trim(v / 1e3) + 'K';
    else s = Number.isInteger(v) ? String(v) : trim(v);
    if (u === '$' || u === '€' || u === '£') s = u + s;
    return s;
  }

  // Title in handwriting (wrapped to two lines at most) with a quick marker underline; returns the y where content starts.
  function title(svg, r, text, sub, pad, f, isNew, accent, W) {
    let top = pad * 0.8;
    if (text) {
      const ft = fit(text, W - 2 * pad, f * 4.4, f * 1.55, f * 1.05, { weight: 700, maxLines: 2 });
      const t = S('text', { x: pad, y: top, 'font-size': ft.size, 'font-weight': 700, fill: INK, 'font-family': HAND }, svg);
      ft.lines.forEach((l, i) => { S('tspan', { x: pad, y: top + ft.size * 0.8 + i * ft.lh }, t).textContent = l; });
      const base = top + ft.size * 0.8 + (ft.lines.length - 1) * ft.lh;
      pen(svg, lineD(r, pad - f * 0.2, base + ft.size * 0.45, pad + ft.w + f * 0.4, base + ft.size * 0.4 + J(r, f * 0.15), f * 0.5), accent, f * 0.22, isNew, 380, 450, { opacity: 0.85 });
      if (isNew) writeIn(t, 0, 380 + text.length * 14);
      top = base + ft.size * 1.0;
    }
    if (sub) {
      const fs = fit(sub, W - 2 * pad, f * 1.4, f * 0.85, f * 0.6, { maxLines: 1 });
      write(svg, fs.lines, pad, top, fs.size, { 'text-anchor': 'start', fill: INK_SOFT }, isNew, 500);
      top += f * 1.3;
    }
    return top;
  }

  // A label for something on a curve: try spots along it, on either side, and take the first that touches no node,
  // no other label and stays on the tile. `at(t)` gives the point and the unit normal at t.
  function placeLabel(at, text, f, W, H, avoid, placed, maxW) {
    const ft = fit(text, maxW || Math.min(W * 0.3, f * 9), f * 3.4, f * 0.82, f * 0.55, { maxLines: 3 });
    const w = ft.w + f * 0.5, h = ft.h + f * 0.3;
    let best = null;
    for (const t of [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74, 0.18, 0.82]) {
      const p = at(t);
      const sides = [-1, 1].map(s => ({ cx: p.x + p.nx * s * (h / 2 + f * 0.3), cy: p.y + p.ny * s * (h / 2 + f * 0.3) })).sort((a, b) => a.cy - b.cy);
      for (const { cx, cy } of sides) {
        const r = { x: cx - w / 2, y: cy - h / 2, w, h };
        if (r.x < 2 || r.y < 2 || r.x + r.w > W - 2 || r.y + r.h > H - 2) continue;
        if (avoid.some(a => overlap(r, a)) || placed.some(a => overlap(r, a))) continue;
        best = r;
        break;
      }
      if (best) break;
    }
    if (!best) { const p = at(0.5); best = { x: p.x - w / 2, y: Math.max(2, p.y - h * 1.6), w, h }; }   // nothing clear: above the middle
    placed.push(best);
    return { x: best.x + best.w / 2, y: best.y + best.h / 2, ft };
  }

  // ---------- diagrams ----------
  // Longest-path layer of every node (a node sits one column right of its furthest predecessor).
  function layers(d) {
    const idx = new Map(d.nodes.map((n, i) => [n.id, i])), L = d.nodes.map(() => 0);
    for (let k = 0; k < d.nodes.length; k++) {
      let ch = false;
      for (const e of d.edges) {
        const a = idx.get(e.from), b = idx.get(e.to);
        if (a === undefined || b === undefined) continue;
        if (L[b] < L[a] + 1 && L[a] + 1 < d.nodes.length) { L[b] = L[a] + 1; ch = true; }
      }
      if (!ch) break;
    }
    if (d.nodes.length > 1 && Math.max(...L) === 0) return d.nodes.map((_, i) => i);    // no edges: a row in the order given
    return L;
  }
  // Order the nodes inside each layer by the mean position of their neighbours (a few sweeps): fewer crossing edges.
  function orderLayers(d, L) {
    const cols = Math.max(...L) + 1, byCol = Array.from({ length: cols }, () => []);
    d.nodes.forEach((nd, i) => byCol[L[i]].push(nd.id));
    const ids = new Set(d.nodes.map(n => n.id)), preds = new Map(), succs = new Map();
    d.nodes.forEach(n => { preds.set(n.id, []); succs.set(n.id, []); });
    for (const e of d.edges) if (ids.has(e.from) && ids.has(e.to)) { succs.get(e.from).push(e.to); preds.get(e.to).push(e.from); }
    const sweep = (col, nbrs) => {
      const at = new Map();
      byCol.forEach(c => c.forEach((id, k) => at.set(id, (k + 0.5) / c.length)));
      const key = new Map(col.map((id, k) => { const ns = nbrs.get(id).filter(x => at.has(x)); return [id, ns.length ? ns.reduce((s, x) => s + at.get(x), 0) / ns.length : (k + 0.5) / col.length]; }));
      col.sort((a, b) => key.get(a) - key.get(b));
    };
    for (let it = 0; it < 4; it++) {
      for (let c = 1; c < cols; c++) sweep(byCol[c], preds);
      for (let c = cols - 2; c >= 0; c--) sweep(byCol[c], succs);
    }
    return byCol;
  }
  // Pull every node toward the average height of the nodes it is linked to (a few sweeps), keeping the order of each
  // column and a minimum gap: edges get shorter and straighter, and their labels get room. Then centre the whole thing.
  function relaxY(byCol, d, pos, top, ah, bh) {
    const adj = new Map(d.nodes.map(n => [n.id, []]));
    for (const e of d.edges) if (adj.has(e.from) && adj.has(e.to)) { adj.get(e.from).push(e.to); adj.get(e.to).push(e.from); }
    const gap = bh * 1.14, lo = top + bh / 2, hi = top + ah - bh / 2;
    // keep a column's order, its minimum gap and the drawing area, all at once
    const settle = ys => {
      const n = ys.length;
      if (n <= 1) return n ? [Math.min(hi, Math.max(lo, ys[0]))] : ys;
      if ((n - 1) * gap > hi - lo) return ys.map((_, k) => lo + ((hi - lo) * k) / (n - 1));   // no room for the full gap: spread evenly
      for (let k = 1; k < n; k++) ys[k] = Math.max(ys[k], ys[k - 1] + gap);
      for (let k = n - 2; k >= 0; k--) ys[k] = Math.min(ys[k], ys[k + 1] - gap);
      return ys.map((y, k) => Math.min(Math.max(y, lo + k * gap), hi - (n - 1 - k) * gap));
    };
    for (let it = 0; it < 8; it++) {
      for (const col of it % 2 ? [...byCol].reverse() : byCol) {
        const want = col.map(id => { const ns = adj.get(id).map(x => pos.get(x).y); return ns.length ? ns.reduce((a, b) => a + b, 0) / ns.length : pos.get(id).y; });
        settle(want).forEach((y, k) => { pos.get(col[k]).y = y; });
      }
    }
    // centre the whole diagram vertically, without pushing anything out of the area
    const all = [...pos.values()].map(p => p.y), lowest = Math.min(...all), highest = Math.max(...all);
    const shift = Math.max(lo - lowest, Math.min(hi - highest, top + ah / 2 - (lowest + highest) / 2));
    for (const p of pos.values()) p.y += shift;
  }
  function port(p, t, bw, bh) {
    const dx = t.x - p.x, dy = t.y - p.y;
    if (Math.abs(dx) * bh > Math.abs(dy) * bw) return { x: p.x + Math.sign(dx) * bw / 2, y: p.y + (dy / (Math.abs(dx) || 1)) * bw / 2 };
    return { x: p.x + (dx / (Math.abs(dy) || 1)) * bh / 2, y: p.y + Math.sign(dy) * bh / 2 };
  }

  function node(g, key, n, i, p, bw, bh, f, isNew, delay, shape, lab, iconS) {
    const r = rng(key), c = MARK[i % MARK.length];
    const x = p.x - bw / 2, y = p.y - bh / 2;
    const gg = S('g', { 'data-box': `${f1(x)},${f1(y)},${f1(bw)},${f1(bh)}` }, g);       // the label must stay inside this
    // colour wash that doesn't quite line up with the outline — like a marker swatch
    const wash = shape === 'ellipse'
      ? S('ellipse', { cx: p.x + f * 0.25, cy: p.y + f * 0.2, rx: bw / 2 * 0.96, ry: bh / 2 * 0.92, fill: c, opacity: 0.16 }, gg)
      : S('rect', { x: x + f * 0.3 + J(r, f * 0.15), y: y + f * 0.25, width: bw * 0.97, height: bh * 0.93, rx: f * 0.25, fill: c, opacity: 0.16,
          transform: `rotate(${f1(J(r, 1.2))} ${p.x} ${p.y})` }, gg);
    if (isNew) anim(wash, [{ opacity: 0 }, { opacity: 0.16 }], delay + 350, 500);
    const outline = shape === 'ellipse' ? ellipseD(r, p.x, p.y, bw / 2, bh / 2, 0.035) : rectD(r, x, y, bw, bh, f * 0.28);
    pen(gg, outline, SOLID ? c : INK, Math.max(1.6, f * 0.13), isNew, delay, 520);
    // icon (or emoji) and label as one block, centred in the box
    const gap = n.icon ? f * 0.25 : 0, block = (n.icon ? iconS + gap : 0) + lab.h, y0 = p.y - block / 2;
    if (n.icon) {
      if (isUrl(n.icon)) picture(gg, n.icon, p.x, y0 + iconS / 2, iconS, isNew, delay + 300);
      else write(gg, n.icon, p.x, y0 + iconS / 2, iconS * 0.85, {}, isNew, delay + 300);
    }
    write(gg, lab.lines, p.x, y0 + (n.icon ? iconS + gap : 0) + lab.h / 2, lab.size, { 'font-weight': 700, lh: lab.lh, broke: lab.broke }, isNew, delay + 380);
    if (n.note) {
      const nf = fit(n.note, bw * 1.15, f * 1.6, f * 0.85, f * 0.6, { weight: 700, maxLines: 1 });
      write(g, nf.lines, p.x, p.y + bh / 2 + nf.size * 0.9, nf.size, { fill: c, 'font-weight': 700, halo: true }, isNew, delay + 520);
    }
  }

  // Bezier helpers for edges (point + unit normal at t).
  function cubicAt(c) {
    return t => {
      const u = 1 - t, x = u * u * u * c.P0.x + 3 * u * u * t * c.C1.x + 3 * u * t * t * c.C2.x + t * t * t * c.P1.x, y = u * u * u * c.P0.y + 3 * u * u * t * c.C1.y + 3 * u * t * t * c.C2.y + t * t * t * c.P1.y;
      const dx = 3 * u * u * (c.C1.x - c.P0.x) + 6 * u * t * (c.C2.x - c.C1.x) + 3 * t * t * (c.P1.x - c.C2.x), dy = 3 * u * u * (c.C1.y - c.P0.y) + 6 * u * t * (c.C2.y - c.C1.y) + 3 * t * t * (c.P1.y - c.C2.y);
      const L = Math.hypot(dx, dy) || 1;
      return { x, y, nx: -dy / L, ny: dx / L };
    };
  }
  function quadAt(P0, C, P1) {
    return t => {
      const u = 1 - t, x = u * u * P0.x + 2 * u * t * C.x + t * t * P1.x, y = u * u * P0.y + 2 * u * t * C.y + t * t * P1.y;
      const dx = 2 * u * (C.x - P0.x) + 2 * t * (P1.x - C.x), dy = 2 * u * (C.y - P0.y) + 2 * t * (P1.y - C.y), L = Math.hypot(dx, dy) || 1;
      return { x, y, nx: -dy / L, ny: dx / L };
    };
  }
  // A flow edge leaves the right side of its source and enters the left side of its target as an S-curve; a
  // backwards edge loops over the top, one within a row runs straight, one within a column runs vertically.
  function flowCurve(a, b, bw, bh, f, r) {
    const gap = f * 0.3;
    let P0, C1, C2, P1;
    if (Math.abs(a.y - b.y) < bh * 0.4 && Math.abs(a.x - b.x) > bw * 0.5) {            // same row: straight across
      const s = Math.sign(b.x - a.x);
      P0 = { x: a.x + s * (bw / 2 + gap), y: a.y }; P1 = { x: b.x - s * (bw / 2 + gap), y: b.y };
      C1 = { x: P0.x + (P1.x - P0.x) / 3, y: P0.y }; C2 = { x: P0.x + (2 * (P1.x - P0.x)) / 3, y: P1.y };
    } else if (b.x - a.x > bw * 0.55) {                                                // forward
      P0 = { x: a.x + bw / 2 + gap, y: a.y }; P1 = { x: b.x - bw / 2 - gap, y: b.y };
      const dx = (P1.x - P0.x) * 0.5; C1 = { x: P0.x + dx, y: P0.y }; C2 = { x: P1.x - dx, y: P1.y };
    } else if (a.x - b.x > bw * 0.55) {                                                // backward: over the top
      P0 = { x: a.x, y: a.y - bh / 2 - gap }; P1 = { x: b.x, y: b.y - bh / 2 - gap };
      const ty = Math.min(P0.y, P1.y) - f * 2.6; C1 = { x: P0.x, y: ty }; C2 = { x: P1.x, y: ty };
    } else {                                                                           // same column
      const s = b.y >= a.y ? 1 : -1;
      P0 = { x: a.x, y: a.y + s * (bh / 2 + gap) }; P1 = { x: b.x, y: b.y - s * (bh / 2 + gap) };
      C1 = { x: P0.x, y: P0.y + (P1.y - P0.y) / 3 }; C2 = { x: P1.x, y: P0.y + (2 * (P1.y - P0.y)) / 3 };
    }
    const w = f * 0.22;
    C1 = { x: C1.x + J(r, w), y: C1.y + J(r, w) }; C2 = { x: C2.x + J(r, w), y: C2.y + J(r, w) };
    return { P0, C1, C2, P1 };
  }
  function flowEdge(g, key, c, f, isNew, delay) {
    const r = rng(key);
    let d = `M${f1(c.P0.x)},${f1(c.P0.y)} C${f1(c.C1.x)},${f1(c.C1.y)} ${f1(c.C2.x)},${f1(c.C2.y)} ${f1(c.P1.x)},${f1(c.P1.y)}`;
    d += ' ' + arrowHeadD(r, c.P1.x, c.P1.y, Math.atan2(c.P1.y - c.C2.y, c.P1.x - c.C2.x) || 0, f * 0.75);
    pen(g, d, INK, Math.max(1.4, f * 0.11), isNew, delay, 500);
  }
  function arrow(g, key, a, b, bw, bh, f, isNew, delay, bow, withHead) {
    const r = rng(key);
    const p = port(a, b, bw, bh), q = port(b, a, bw, bh);
    const gap = f * 0.35, L = Math.hypot(q.x - p.x, q.y - p.y) || 1, ux = (q.x - p.x) / L, uy = (q.y - p.y) / L;
    const sx = p.x + ux * gap, sy = p.y + uy * gap, ex = q.x - ux * gap, ey = q.y - uy * gap;
    const bo = (bow || 0) + J(r, f * 0.4);
    const cx = (sx + ex) / 2 - uy * bo, cy = (sy + ey) / 2 + ux * bo;
    let d = `M${f1(sx + J(r, 1))},${f1(sy + J(r, 1))} Q${f1(cx)},${f1(cy)} ${f1(ex)},${f1(ey)}`;
    if (withHead) d += ' ' + arrowHeadD(r, ex, ey, Math.atan2(ey - cy, ex - cx), f * 0.75);
    pen(g, d, INK, Math.max(1.4, f * 0.11), isNew, delay, 500);
    return quadAt({ x: sx, y: sy }, { x: cx, y: cy }, { x: ex, y: ey });
  }

  function renderDiagram(svg, el, d, W, H, seen, f) {
    const pad = f * 1.8, rT = rng(el.id + ':title');
    const isNewTitle = !seen.has('title:' + d.title);
    seen.add('title:' + d.title);
    const top = title(svg, rT, d.title, null, pad, f, isNewTitle, MARK[1], W);
    const n = d.nodes.length, aw = W - 2 * pad, ah = H - top - pad * 0.8;
    const g = S('g', {}, svg), pos = new Map();
    const hasIcon = d.nodes.some(x => x.icon);
    let bw, bh = f * (hasIcon ? 5.4 : 3.8), labelW;
    const cxm = pad + aw / 2, cym = top + ah / 2;
    let shape = 'box';

    if (d.layout === 'cycle' || d.layout === 'hub') {
      const ring = d.layout === 'hub' ? d.nodes.slice(1) : d.nodes;
      bw = Math.min(f * 10, aw / Math.max(3, Math.ceil(ring.length / 2) + 1.2));
      bh = Math.min(bh, ah / 3.2);
      const rx = Math.max(bw * 0.8, aw / 2 - bw / 2 - f * 0.4), ry = Math.max(bh * 0.8, ah / 2 - bh / 2 - f * 0.2);
      ring.forEach((nd, i) => { const a = -Math.PI / 2 + (2 * Math.PI * i) / Math.max(1, ring.length); pos.set(nd.id, { x: cxm + rx * Math.cos(a), y: cym + ry * Math.sin(a) }); });
      if (d.layout === 'hub') pos.set(d.nodes[0].id, { x: cxm, y: cym });
      shape = d.layout === 'cycle' ? 'ellipse' : 'box';
    } else if (d.layout === 'timeline') {
      const y = top + ah * 0.38, r = rng(el.id + ':axis'), isNew = !seen.has('axis');
      seen.add('axis');
      pen(g, lineD(r, pad * 0.6, y, W - pad * 0.6, y, f * 0.3) + ' ' + arrowHeadD(r, W - pad * 0.6, y, 0, f * 0.8), INK, f * 0.14, isNew, 0, 700);
      const cell = aw / n, room = Math.max(f * 3, H - pad * 0.6 - (y + f * 1.4));
      const trial = d.nodes.map(nd => fit(nd.label, cell * 0.92, room, f * 1.0, f * 0.6, { weight: 700, maxLines: 4 })), uni = Math.min(...trial.map(t => t.size));
      const noteS = Math.min(f * 1.35, ...d.nodes.filter(nd => nd.note).map(nd => fit(nd.note, cell * 0.92, f * 2.2, f * 1.35, f * 0.7, { weight: 700, maxLines: 1 }).size));
      d.nodes.forEach((nd, i) => {
        const x = pad + cell * (i + 0.5), key = 'n:' + nd.label, isN = !seen.has(key), delay = isN ? 250 + i * 120 : 0, c = MARK[i % MARK.length];
        seen.add(key);
        const rr = rng(el.id + key);
        pen(g, lineD(rr, x, y - f * 0.5, x, y + f * 0.5, 0.4), INK, f * 0.14, isN, delay, 200);
        pen(g, ellipseD(rr, x, y, f * 0.35, f * 0.35, 0.08), c, f * 0.16, isN, delay + 100, 260);
        if (nd.note) write(g, fit(nd.note, cell * 0.92, f * 2.2, noteS, noteS, { weight: 700, maxLines: 1 }).lines, x, y - f * 1.5, noteS, { fill: c, 'font-weight': 700 }, isN, delay + 150);
        const lb = fit(nd.label, cell * 0.92, room, uni, uni, { weight: 700, maxLines: 4 });
        let ly = y + f * 1.9 + lb.h / 2;
        if (nd.icon) { const s = Math.min(f * 2, cell * 0.5); if (isUrl(nd.icon)) picture(g, nd.icon, x, y + f * 1.2 + s / 2, s, isN, delay + 200); ly += s + f * 0.3; }
        write(g, lb.lines, x, ly, lb.size, { 'font-weight': 700, lh: lb.lh }, isN, delay + 300);
      });
      return;
    } else {
      const L = layers(d), cols = Math.max(...L) + 1;
      if (cols === n && n > 4 && aw / n < f * 8) {           // long chain → two rows, second runs back
        const perRow = Math.ceil(n / 2);
        bw = Math.min(f * 10, (aw / perRow) * 0.76);
        d.nodes.forEach((nd, i) => {
          const row = i < perRow ? 0 : 1, slot = row ? perRow - 1 - (i - perRow) : i;
          pos.set(nd.id, { x: pad + (aw / perRow) * (slot + 0.5), y: top + (ah / 2) * (row + 0.5) });
        });
        bh = Math.min(bh, (ah / 2) * 0.72);
      } else {
        const byCol = orderLayers(d, L);
        // One text scale for the whole diagram: the largest at which every box holds its longest word and every gap
        // holds its edge labels (written between the columns). A crowded diagram gets smaller text, not clipped text.
        const wordW = (t, size, wt) => TextFit.longestWord(t, size, wt, HAND);
        let s = 1, need;
        for (; s >= 0.4; s -= 0.04) {
          const g = f * s, iconW = hasIcon ? g * 2.2 : 0;
          const bwN = Math.max(g * 4, iconW, ...d.nodes.map(nd => wordW(nd.label, g * 0.74, 700) + g * 1.0));
          const gapN = d.edges.some(e => e.label) ? Math.max(g * 2, ...d.edges.filter(e => e.label).map(e => Math.max(wordW(e.label, g * 0.58, 400), TextFit.width(e.label, g * 0.58, 400, HAND) / 3)) ) + g * 0.9 : g * 1.8;
          need = { bwN, gapN };
          if (cols * bwN + (cols - 1) * gapN <= aw) break;
        }
        s = Math.max(s, 0.4);
        f = f * s;                                               // everything below (text, strokes, spacing) follows
        bw = cols > 1 ? Math.min(f * 12, Math.max(need.bwN, (aw - (cols - 1) * need.gapN) / cols)) : Math.min(f * 12, aw * 0.6);
        labelW = cols > 1 ? (aw - cols * bw) / (cols - 1) - f * 0.5 : undefined;
        byCol.forEach((list, c) => list.forEach((id, k) => pos.set(id, { x: cols > 1 ? pad + bw / 2 + ((aw - bw) / (cols - 1)) * c : pad + aw / 2, y: top + (ah / list.length) * (k + 0.5) })));
        bh = Math.min(bh, (ah / Math.max(...byCol.map(l => l.length))) * 0.8);
        relaxY(byCol, d, pos, top, ah, bh);
      }
    }

    // Labels: one size for the whole diagram (the smallest that lets every label fit its box), so it looks tidy.
    const iconS = hasIcon ? Math.max(f * 1.1, Math.min(f * 2.1, bh * 0.36)) : 0;
    const boxOf = (i) => { const hub = d.layout === 'hub' && i === 0; return { hub, w: hub ? bw * 1.15 : bw, h: hub ? bh * 1.15 : bh, ell: hub || shape === 'ellipse' }; };
    const inner = (nd, i) => { const b = boxOf(i); return [(b.ell ? b.w * 0.8 : b.w) - f * 1.0, (b.ell ? b.h * 0.78 : b.h) - f * 0.5 - (nd.icon ? iconS + f * 0.25 : 0)]; };
    const trial = d.nodes.map((nd, i) => { const [iw, ih] = inner(nd, i); return fit(nd.label, iw, ih, f * 1.05, f * 0.6, { weight: 700, maxLines: 3 }); });
    const uni = Math.min(...trial.map(t => t.size));
    const labs = d.nodes.map((nd, i) => { const [iw, ih] = inner(nd, i); return fit(nd.label, iw, ih, uni, uni, { weight: 700, maxLines: 3 }); });

    let k = 0;
    const delayOf = new Map();
    d.nodes.forEach(nd => { if (!seen.has('n:' + nd.label)) delayOf.set(nd.id, 180 * k++); });
    // where labels may not go: the title, every node (and the note under it)
    const avoid = [{ x: 0, y: 0, w: W, h: top }];
    d.nodes.forEach((nd, i) => { const p = pos.get(nd.id), b = boxOf(i); if (p) { avoid.push({ x: p.x - b.w / 2 - f * 0.2, y: p.y - b.h / 2 - f * 0.2, w: b.w + f * 0.4, h: b.h + f * 0.4 + (nd.note ? f * 1.6 : 0) }); } });
    const placed = [], eg = S('g', {}, g), labels = [];
    for (const e of d.edges) {
      const a = pos.get(e.from), b = pos.get(e.to);
      if (!a || !b) continue;
      const key = 'e:' + e.from + '>' + e.to, isNew = !seen.has(key), delay = (delayOf.get(e.to) || 0) + 420;
      seen.add(key);
      let at;
      if (d.layout === 'flow') {
        const c = flowCurve(a, b, bw, bh, f, rng(el.id + key));
        flowEdge(eg, el.id + key, c, f, isNew, delay);
        at = cubicAt(c);
      } else at = arrow(eg, el.id + key, a, b, bw, bh, f, isNew, delay, d.layout === 'cycle' ? f * 1.6 : 0, d.layout !== 'hub');
      if (e.label) labels.push({ at, text: e.label, isNew, delay });
    }
    for (const l of labels) {
      const p = placeLabel(l.at, l.text, f, W, H, avoid, placed, labelW);
      write(eg, p.ft.lines, p.x, p.y, p.ft.size, { fill: INK_SOFT, halo: true, lh: p.ft.lh, broke: p.ft.broke }, l.isNew, l.delay + 300);
    }
    d.nodes.forEach((nd, i) => {
      const p = pos.get(nd.id);
      if (!p) return;
      const key = 'n:' + nd.label, isNew = !seen.has(key), b = boxOf(i);
      seen.add(key);
      node(g, el.id + key, nd, i, p, b.w, b.h, f, isNew, delayOf.get(nd.id) || 0, b.hub ? 'ellipse' : shape, labs[i], iconS);
    });
  }

  // ---------- charts ----------
  function renderChart(svg, el, c, W, H, seen, f) {
    const pad = f * 1.8, pts = c.points;
    const unitSub = c.unit && !['%', '$', '€', '£'].includes(c.unit.trim()) ? c.unit : null;
    const tNew = !seen.has('title:' + c.title);
    seen.add('title:' + c.title);
    const single = c.kind === 'stat' && pts.length === 1 || pts.length === 1;
    const top = title(svg, rng(el.id + ':title'), c.title, single ? null : unitSub, pad, f, tNew, MARK[0], W);
    const aw = W - 2 * pad, ah = H - top - pad * 0.7;
    const g = S('g', {}, svg);
    const fresh = p => { const key = 'p:' + p.label + '=' + p.value, n = !seen.has(key); seen.add(key); return n; };
    const defs = S('defs', {}, svg);
    let clipN = 0;
    const clip = d => { const id = `ck${el.id}-${++clipN}-${Math.round(performance.now())}`; S('path', { d }, S('clipPath', { id }, defs)); return `url(#${id})`; };

    if (c.kind === 'stat' || pts.length === 1) {
      const cx = pad + aw / 2, cy = top + ah * 0.45;
      if (pts.length >= 2) {
        const a = pts[0], b = pts[pts.length - 1], colW = aw * 0.4, big = Math.min(f * 4.6, aw / 7);
        const isN = fresh(a) | fresh(b), r = rng(el.id + ':stat');
        const va = fit(fmt(a.value, c.unit), colW, big * 1.3, big * 0.75, f * 1.2, { weight: 700, maxLines: 1 }), vb = fit(fmt(b.value, c.unit), colW, big * 1.5, big, f * 1.2, { weight: 700, maxLines: 1 });
        const la = fit(a.label, colW, f * 2.6, f * 1.0, f * 0.65, { maxLines: 2 }), lb = fit(b.label, colW, f * 2.6, f * 1.0, f * 0.65, { maxLines: 2 });
        write(g, va.lines, cx - aw * 0.27, cy, va.size, { fill: INK_SOFT, 'font-weight': 700 }, isN, 0);
        write(g, la.lines, cx - aw * 0.27, cy + big * 0.8 + la.h / 2, la.size, { fill: INK_SOFT, lh: la.lh }, isN, 200);
        pen(g, `M${f1(cx - aw * 0.12)},${f1(cy)} Q${f1(cx)},${f1(cy - f * 1.6)} ${f1(cx + aw * 0.08)},${f1(cy)} ` + arrowHeadD(r, cx + aw * 0.08, cy, 0.5, f * 0.9), INK, f * 0.16, isN, 450, 450);
        write(g, vb.lines, cx + aw * 0.25, cy, vb.size, { 'font-weight': 700 }, isN, 800);
        write(g, lb.lines, cx + aw * 0.25, cy + big * 0.8 + lb.h / 2, lb.size, { fill: INK_SOFT, lh: lb.lh }, isN, 950);
        if (a.value) {
          const ch = ((b.value - a.value) / Math.abs(a.value)) * 100;
          const s = (ch >= 0 ? '+' : '') + (Math.abs(ch) >= 100 ? Math.round(ch) : ch.toFixed(1).replace(/\.0$/, '')) + '%';
          const col = ch >= 0 ? MARK[3] : MARK[0], by = cy + big * 1.55 + f * 0.6;
          write(g, s, cx, by, f * 1.5, { fill: col, 'font-weight': 700 }, isN, 1200);
          pen(g, ellipseD(r, cx, by, tw(s, f * 1.5, 700) / 2 + f * 0.7, f * 1.15, 0.05), col, f * 0.16, isN, 1350, 550);
        }
        return;
      }
      const p = pts[0], r = rng(el.id + ':stat1'), isN = fresh(p);
      const s = fmt(p.value, c.unit), unitWord = unitSub ? String(c.unit).trim() : '';
      const uf = f * 1.7, ugap = f * 0.6;
      const uw = unitWord ? Math.min(tw(unitWord, uf, 700), aw * 0.3) : 0;
      const big = fit(s, aw * 0.85 - (unitWord ? uw + ugap : 0), f * 9, Math.min(f * 6.5, aw / 4.5), f * 1.5, { weight: 700, maxLines: 1 }).size;
      const numW = tw(s, big, 700), total = numW + (unitWord ? ugap + uw : 0), x0 = cx - total / 2;
      write(g, s, x0 + numW / 2, cy, big, { 'font-weight': 700 }, isN, 0);
      if (unitWord) write(g, fit(unitWord, uw + 1, f * 2.4, uf, f * 0.8, { weight: 700, maxLines: 1 }).lines, x0 + numW + ugap, cy + big * 0.18, uf, { 'text-anchor': 'start', fill: INK_SOFT, 'font-weight': 700 }, isN, 250);
      const w = Math.max(big * 1.1, total);
      pen(g, lineD(r, cx - w / 2, cy + big * 0.55, cx + w / 2, cy + big * 0.5, f * 0.5) + ' ' + lineD(r, cx - w / 2 + f, cy + big * 0.66, cx + w / 2 - f * 0.5, cy + big * 0.62, f * 0.5), MARK[0], f * 0.22, isN, 500, 500);
      const lab = fit(p.label, aw * 0.85, Math.max(f * 2, top + ah - (cy + big * 0.95)), f * 1.2, f * 0.7, { maxLines: 2 });
      write(g, lab.lines, cx, cy + big * 0.95 + lab.h / 2, lab.size, { fill: INK_SOFT, lh: lab.lh }, isN, 700);
      return;
    }

    if (c.kind === 'pie') {
      const total = pts.reduce((s, p) => s + Math.max(0, p.value), 0) || 1, n = pts.length;
      // The pie sits left and a legend runs down the right (a narrow tile stacks them): a label can never leave the tile
      // or collide with another, however long it is.
      const sideW = aw - 2 * Math.min(ah * 0.42, aw * 0.27) - f * 2.2, stacked = sideW < f * 10;
      const R = stacked ? Math.min(aw * 0.3, ah * 0.3) : Math.min(ah * 0.42, aw * 0.27);
      const cx = stacked ? pad + aw / 2 : pad + R + f * 0.4, cy = stacked ? top + R + f * 0.3 : top + ah / 2;
      const L = stacked ? { x: pad, y: cy + R + f * 1.0, w: aw, h: top + ah - (cy + R + f * 1.0) } : { x: cx + R + f * 2.2, y: top, w: W - pad - (cx + R + f * 2.2), h: ah };
      const pctText = (p, share) => c.unit && c.unit.trim() === '%' ? fmt(p.value, '%') : Math.round(share * 100) + '%';
      const shares = pts.map(p => Math.max(0, p.value) / total), pcts = pts.map((p, i) => pctText(p, shares[i]));
      const rowH = Math.max(f * 1.3, Math.min(f * 3.4, L.h / n)), hasIcon = pts.some(p => p.icon), iconS = hasIcon ? Math.min(f * 1.9, rowH * 0.86) : 0;
      const swatch = f * 1.0, pctSize = Math.min(f * 1.15, rowH * 0.62), pctW = Math.max(...pcts.map(s => tw(s, pctSize, 700))) + f * 0.3;
      const labW = Math.max(f * 3, L.w - swatch - f * 0.7 - (iconS ? iconS + f * 0.5 : 0) - pctW - f * 0.4);
      const trial = pts.map(p => fit(p.label, labW, rowH * 0.96, f * 1.05, f * 0.6, { maxLines: 2 })), uni = Math.min(...trial.map(t => t.size));
      const labs = pts.map(p => fit(p.label, labW, rowH * 0.96, uni, uni, { maxLines: 2 }));
      const y0 = Math.max(L.y, L.y + (L.h - rowH * n) / 2);
      let acc = -Math.PI / 2, k = 0;
      pts.forEach((p, i) => {
        const a0 = acc, a1 = acc + shares[i] * Math.PI * 2, col = MARK[i % MARK.length];
        acc = a1;
        const r = rng(el.id + ':w:' + p.label), isN = fresh(p), delay = isN ? 200 * k++ : 0;
        const arc = [], steps = Math.max(3, Math.ceil(shares[i] * 24));
        for (let s = 0; s <= steps; s++) { const t = a0 + ((a1 - a0) * s) / steps, rr = R * (1 + J(r, 0.02)); arc.push([cx + rr * Math.cos(t), cy + rr * Math.sin(t)]); }
        const wedge = `M${cx},${cy} L${f1(arc[0][0])},${f1(arc[0][1])} ` + smooth(arc).replace(/^M[^C]*/, '') + ' Z';
        const wash = S('path', { d: wedge, fill: col, opacity: SOLID ? 0.42 : 0.18 }, g);
        if (!SOLID) pen(g, hatchD(r, cx - R, cy - R, 2 * R, 2 * R, f * 0.55 + i * 1.5, f * 0.2), col, f * 0.1, isN, delay + 250, 700, { 'clip-path': clip(wedge), opacity: 0.75 });
        pen(g, lineD(r, cx, cy, arc[0][0], arc[0][1], f * 0.2) + ' ' + smooth(arc), INK, f * 0.13, isN, delay, 600);
        if (isN) anim(wash, [{ opacity: 0 }, { opacity: SOLID ? 0.42 : 0.18 }], delay + 200, 500);
        // legend row: colour chip, (icon), label, share
        const ry = y0 + rowH * (i + 0.5), rr = rng(el.id + ':chip:' + p.label);
        let x = L.x;
        pen(g, ellipseD(rr, x + swatch / 2, ry, swatch * 0.42, swatch * 0.42, 0.08), col, f * 0.16, isN, delay + 400, 300);
        S('circle', { cx: f1(x + swatch / 2 + f * 0.1), cy: f1(ry + f * 0.08), r: f1(swatch * 0.36), fill: col, opacity: 0.32 }, g);
        x += swatch + f * 0.7;
        if (iconS && p.icon) { if (isUrl(p.icon)) picture(g, p.icon, x + iconS / 2, ry, iconS, isN, delay + 450); x += iconS + f * 0.5; }
        write(g, labs[i].lines, x, ry, labs[i].size, { 'text-anchor': 'start', 'font-weight': 700, lh: labs[i].lh }, isN, delay + 500);
        write(g, pcts[i], L.x + L.w, ry, pctSize, { 'text-anchor': 'end', 'font-weight': 700 }, isN, delay + 550);
      });
      // circle the biggest share, like a presenter marking the headline number
      const li = shares.indexOf(Math.max(...shares)), rk = rng(el.id + ':key:' + pts[li].label);
      const isNk = !seen.has('key:' + pts[li].label + pts[li].value);
      seen.add('key:' + pts[li].label + pts[li].value);
      const kw = tw(pcts[li], pctSize, 700) + f * 1.0;
      pen(g, ellipseD(rk, L.x + L.w - kw / 2 + f * 0.3, y0 + rowH * (li + 0.5), kw * 0.6, Math.min(f * 1.4, rowH * 0.55), 0.05), MARK[li % MARK.length], f * 0.15, isNk, 1100, 600, { opacity: 0.9 });
      return;
    }

    // bar & line: a hand-drawn baseline; icons and labels under it, sized to fit their slot
    const x0 = pad, x1 = W - pad, slot = (x1 - x0) / pts.length, X = i => x0 + slot * (i + 0.5);
    const hasIcon = pts.some(p => p.icon), iconS = hasIcon ? Math.min(f * 2.0, slot * 0.6) : 0;
    const trial = pts.map(p => fit(p.label, slot * 0.94, f * 3.6, f * 1.0, f * 0.6, { maxLines: 3 })), uni = Math.min(...trial.map(t => t.size));
    const labs = pts.map(p => fit(p.label, slot * 0.94, f * 3.6, uni, uni, { maxLines: 3 })), labH = Math.max(...labs.map(l => l.h));
    const below = (hasIcon ? iconS + f * 0.4 : 0) + labH + f * 0.7;
    const y1 = H - pad * 0.6 - below, y0 = top + f * 2.4;
    const Y = v => y1 - (Math.max(0, v) / (Math.max(...pts.map(p => p.value), 0) || 1)) * (y1 - y0);
    const rb = rng(el.id + ':base');
    pen(g, lineD(rb, x0 - f * 0.4, y1, x1 + f * 0.4, y1 + J(rb, 1), f * 0.4), INK, f * 0.15, !seen.has('base'), 0, 500);
    seen.add('base');
    pts.forEach((p, i) => {
      const key = 'x:' + p.label, isN = !seen.has(key);
      seen.add(key);
      let yy = y1 + f * 0.85;
      if (p.icon && isUrl(p.icon)) { picture(g, p.icon, X(i), yy + iconS / 2, iconS, isN, 150 + i * 120); yy += iconS + f * 0.4; }
      write(g, labs[i].lines, X(i), yy + labs[i].h / 2, labs[i].size, { fill: INK_SOFT, lh: labs[i].lh }, isN, 150 + i * 120);
    });
    const valueLabel = (p, i, size) => fit(fmt(p.value, c.unit), slot * 0.98, f * 2, size, f * 0.8, { weight: 700, maxLines: 1 });

    if (c.kind === 'line') {
      const r = rng(el.id + ':line'), newPts = pts.map(fresh);
      let d = '';
      for (let i = 0; i < pts.length - 1; i++) d += ' ' + lineD(r, X(i), Y(pts[i].value), X(i + 1), Y(pts[i + 1].value), f * 0.35);
      pen(g, d.trim(), MARK[1], f * 0.22, newPts.some(Boolean), 200, 300 * pts.length);
      pts.forEach((p, i) => {
        const rr = rng(el.id + ':pt:' + p.label), vl = valueLabel(p, i, f * 1.2);
        pen(g, ellipseD(rr, X(i), Y(p.value), f * 0.38, f * 0.38, 0.08), MARK[1], f * 0.18, newPts[i], 200 + i * 300, 250);
        // above the point, unless it is a dip (both neighbours higher) or the label would leave the chart
        const dip = (i === 0 || pts[i - 1].value > p.value) && (i === pts.length - 1 || pts[i + 1].value > p.value) && pts.length > 1;
        let ly = Y(p.value) + (dip ? 1 : -1) * f * 1.3;
        if (ly < top + f * 0.9) ly = Y(p.value) + f * 1.3;
        if (ly > y1 - f * 0.6) ly = Y(p.value) - f * 1.3;
        write(g, vl.lines, X(i), ly, vl.size, { 'font-weight': 700, halo: true }, newPts[i], 350 + i * 300);
      });
      return;
    }

    const bw = Math.min(slot * 0.56, f * 6.5);
    let k = 0;
    pts.forEach((p, i) => {
      const col = MARK[i % MARK.length], r = rng(el.id + ':bar:' + p.label), isN = fresh(p), delay = isN ? 220 * k++ : 0;
      const x = X(i) - bw / 2, y = Y(p.value), h = Math.max(2, y1 - y), vl = valueLabel(p, i, f * 1.35);
      const wash = S('rect', { x: x + f * 0.25, y: y + f * 0.2, width: bw, height: Math.max(1, h - f * 0.2), fill: col, opacity: SOLID ? 0.5 : 0.2, transform: `rotate(${f1(J(r, 0.8))} ${X(i)} ${y1})` }, g);
      const box = `M${x},${y1} L${x},${y} L${x + bw},${y} L${x + bw},${y1} Z`;
      if (!SOLID) pen(g, hatchD(r, x, y, bw, h, f * 0.5, f * 0.2), col, f * 0.11, isN, delay + 350, Math.min(900, 300 + h), { 'clip-path': clip(box), opacity: 0.8 });
      pen(g, lineD(r, x, y1, x, y, f * 0.25) + ' ' + lineD(r, x, y, x + bw, y, f * 0.25) + ' ' + lineD(r, x + bw, y, x + bw, y1, f * 0.25), SOLID ? col : INK, f * 0.13, isN, delay, 550);
      if (isN) anim(wash, [{ opacity: 0 }, { opacity: SOLID ? 0.5 : 0.2 }], delay + 300, 500);
      write(g, vl.lines, X(i), Math.max(top + f * 0.9, y - f * 1.1), vl.size, { 'font-weight': 700 }, isN, delay + 500);
    });
  }

  // ---------- annotations (used by index.html) ----------
  function circleAround(svg, key, W, H, label, isNew) {
    svg.innerHTML = '';
    svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    const r = rng(key), f = Math.max(10, Math.min(W, H) * 0.03);
    pen(svg, ellipseD(r, W / 2, H / 2, W * 0.46, H * 0.47, 0.03), MARK[0], f * 0.3, isNew, 0, 700, { opacity: 0.9 });
    if (label) write(svg, label, W * 0.5, H - f * 0.4, f * 1.5, { fill: MARK[0], 'font-weight': 700 }, isNew, 600);
  }
  function arrowBetween(g, key, ax, ay, bx, by, f, label, isNew) {
    const r = rng(key), mx = (ax + bx) / 2, my = Math.min(ay, by) - f * 4;
    pen(g, `M${f1(ax)},${f1(ay)} Q${f1(mx)},${f1(my)} ${f1(bx)},${f1(by)} ` + arrowHeadD(r, bx, by, Math.atan2(by - my, bx - mx), f * 1.2), MARK[0], f * 0.3, isNew, 0, 700);
    if (label) write(g, label, mx, my + f * 1.2, f * 1.6, { fill: MARK[0], 'font-weight': 700 }, isNew, 500);
  }

  window.Sketch = {
    render(svg, el, W, H, seen, full, theme) {
      applyTheme(theme);
      svg.innerHTML = '';
      svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
      const f = Math.max(12, Math.min(W, H * 1.6) * (full ? 0.03 : 0.026));
      if (el.diagram) renderDiagram(svg, el, el.diagram, W, H, seen, f);
      else if (el.chart) renderChart(svg, el, el.chart, W, H, seen, f);
    },
    circleAround, arrowBetween, rng, fmt,
  };
})();
