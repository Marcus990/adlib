// "Live sketch" theme (LS_THEME=sketch, the default): diagrams and charts look drawn by hand while the
// presenter talks — marker strokes that draw themselves, handwritten labels, hatched fills on warm paper.
// Same API as graphics.js: Sketch.render(svg, element, W, H, seen, full). Randomness is seeded per
// element/node so a graphic never re-wobbles when the board re-renders; only new parts animate in.
(function () {
  const NS = 'http://www.w3.org/2000/svg';
  const INK = '#2b2723', INK_SOFT = '#6f665b';
  const MARK = ['#d1495b', '#2e86ab', '#e59b2f', '#3c9d5d', '#7b5ea7', '#d45d9b', '#1f7a8c', '#a0673a'];
  const HAND = '"Noteworthy", "Chalkboard SE", "Marker Felt", cursive';

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
  const J = (r, a) => (r() - 0.5) * 2 * a;
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
    const n = 20, start = r() * Math.PI * 2, over = 0.12 + r() * 0.14, pts = [];
    for (let i = 0; i <= n; i++) {
      const t = start + (Math.PI * 2 * (1 + over) * i) / n, k = 1 + J(r, wobble || 0.04) + (i / n) * 0.03;
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
  function wrap(text, maxW, size, maxLines) {
    const per = Math.max(4, Math.floor(maxW / (size * 0.5)));
    const words = String(text).split(/\s+/).filter(Boolean), lines = [];
    let cur = '';
    for (const w of words) {
      if (!cur) cur = w;
      else if ((cur + ' ' + w).length <= per) cur += ' ' + w;
      else { lines.push(cur); cur = w; }
    }
    if (cur) lines.push(cur);
    if (lines.length > maxLines) { const k = lines.slice(0, maxLines); k[maxLines - 1] = k[maxLines - 1].slice(0, per - 1) + '…'; return k; }
    return lines;
  }
  function write(g, lines, x, y, size, attrs, isNew, delay) {
    const t = S('text', Object.assign({ x, y, 'font-size': size, fill: INK, 'text-anchor': 'middle', 'dominant-baseline': 'middle',
      'font-family': HAND }, attrs), g);
    const lh = size * 1.12;
    (Array.isArray(lines) ? lines : [lines]).forEach((l, i, a) => S('tspan', { x, dy: i === 0 ? -((a.length - 1) * lh) / 2 : lh }, t).textContent = l);
    if (isNew) writeIn(t, delay, 280 + String(lines).length * 18);
    return t;
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

  // Title in handwriting with a quick marker underline; returns the y where content starts.
  function title(svg, r, text, sub, pad, f, isNew, accent) {
    let top = pad * 0.8;
    if (text) {
      const size = f * 1.55;
      const t = S('text', { x: pad, y: top + size * 0.8, 'font-size': size, 'font-weight': 700, fill: INK, 'font-family': HAND }, svg);
      t.textContent = text;
      const w = Math.min(text.length * size * 0.5, 9999);
      pen(svg, lineD(r, pad - f * 0.2, top + size * 1.25, pad + w + f * 0.4, top + size * 1.2 + J(r, f * 0.15), f * 0.5), accent, f * 0.22, isNew, 380, 450, { opacity: 0.85 });
      if (isNew) writeIn(t, 0, 380 + text.length * 14);
      top += size * 1.8;
    }
    if (sub) { write(svg, sub, pad, top, f * 0.85, { 'text-anchor': 'start', fill: INK_SOFT }, isNew, 500); top += f * 1.3; }
    return top;
  }

  // ---------- diagrams ----------
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
    return L;
  }
  function port(p, t, bw, bh) {
    const dx = t.x - p.x, dy = t.y - p.y;
    if (Math.abs(dx) * bh > Math.abs(dy) * bw) return { x: p.x + Math.sign(dx) * bw / 2, y: p.y + (dy / (Math.abs(dx) || 1)) * bw / 2 };
    return { x: p.x + (dx / (Math.abs(dy) || 1)) * bh / 2, y: p.y + Math.sign(dy) * bh / 2 };
  }

  function node(g, key, n, i, p, bw, bh, f, isNew, delay, shape) {
    const r = rng(key), c = MARK[i % MARK.length], gg = S('g', {}, g);
    const x = p.x - bw / 2, y = p.y - bh / 2;
    // colour wash that doesn't quite line up with the outline — like a marker swatch
    const wash = shape === 'ellipse'
      ? S('ellipse', { cx: p.x + f * 0.25, cy: p.y + f * 0.2, rx: bw / 2 * 0.96, ry: bh / 2 * 0.92, fill: c, opacity: 0.16 }, gg)
      : S('rect', { x: x + f * 0.3 + J(r, f * 0.15), y: y + f * 0.25, width: bw * 0.97, height: bh * 0.93, rx: f * 0.25, fill: c, opacity: 0.16,
          transform: `rotate(${f1(J(r, 1.2))} ${p.x} ${p.y})` }, gg);
    if (isNew) anim(wash, [{ opacity: 0 }, { opacity: 0.16 }], delay + 350, 500);
    const outline = shape === 'ellipse' ? ellipseD(r, p.x, p.y, bw / 2, bh / 2, 0.035) : rectD(r, x, y, bw, bh, f * 0.28);
    pen(gg, outline, INK, Math.max(1.6, f * 0.13), isNew, delay, 520);
    const size = f * 1.05, lh = size * 1.12;
    const lines = wrap(n.label, bw - f * 1.2, size, 2);
    const iconH = n.icon ? f * 1.75 : 0, y0 = p.y - (iconH + lines.length * lh) / 2;   // icon + label as one centred block
    if (n.icon) write(gg, n.icon, p.x, y0 + iconH * 0.45, f * 1.4, {}, isNew, delay + 300);
    write(gg, lines, p.x, y0 + iconH + (lines.length * lh) / 2, size, { 'font-weight': 700 }, isNew, delay + 380);
    if (n.note) write(gg, n.note, p.x, p.y + bh / 2 + f * 0.9, f * 0.85, { fill: c, 'font-weight': 700 }, isNew, delay + 520);
  }

  function arrow(g, key, a, b, bw, bh, f, isNew, delay, bow, withHead, label) {
    const r = rng(key);
    const p = port(a, b, bw, bh), q = port(b, a, bw, bh);
    const gap = f * 0.35, L = Math.hypot(q.x - p.x, q.y - p.y) || 1, ux = (q.x - p.x) / L, uy = (q.y - p.y) / L;
    const sx = p.x + ux * gap, sy = p.y + uy * gap, ex = q.x - ux * gap, ey = q.y - uy * gap;
    const bo = (bow || 0) + J(r, f * 0.4);
    const cx = (sx + ex) / 2 - uy * bo, cy = (sy + ey) / 2 + ux * bo;
    let d = `M${f1(sx + J(r, 1))},${f1(sy + J(r, 1))} Q${f1(cx)},${f1(cy)} ${f1(ex)},${f1(ey)}`;
    if (withHead) d += ' ' + arrowHeadD(r, ex, ey, Math.atan2(ey - cy, ex - cx), f * 0.75);
    pen(g, d, INK, Math.max(1.4, f * 0.11), isNew, delay, 500);
    if (label) write(g, label, (sx + 2 * cx + ex) / 4, (sy + 2 * cy + ey) / 4 - f * 0.7, f * 0.8, { fill: INK_SOFT }, isNew, delay + 300);
  }

  function renderDiagram(svg, el, d, W, H, seen, f) {
    const pad = f * 1.8, rT = rng(el.id + ':title');
    const isNewTitle = !seen.has('title:' + d.title);
    seen.add('title:' + d.title);
    const top = title(svg, rT, d.title, null, pad, f, isNewTitle, MARK[1]);
    const n = d.nodes.length, aw = W - 2 * pad, ah = H - top - pad * 0.8;
    const g = S('g', {}, svg), pos = new Map();
    let bw, bh = f * (d.nodes.some(x => x.icon) ? 4.8 : 3.4);
    const cxm = pad + aw / 2, cym = top + ah / 2;
    let shape = 'box';

    if (d.layout === 'cycle' || d.layout === 'hub') {
      const ring = d.layout === 'hub' ? d.nodes.slice(1) : d.nodes;
      bw = Math.min(f * 9, aw / Math.max(3, Math.ceil(ring.length / 2) + 1.2));
      bh = Math.min(bh, ah / 3.2);
      const rx = Math.max(bw * 0.8, aw / 2 - bw / 2 - f * 0.4), ry = Math.max(bh * 0.8, ah / 2 - bh / 2 - f * 0.2);
      ring.forEach((nd, i) => { const a = -Math.PI / 2 + (2 * Math.PI * i) / Math.max(1, ring.length); pos.set(nd.id, { x: cxm + rx * Math.cos(a), y: cym + ry * Math.sin(a) }); });
      if (d.layout === 'hub') pos.set(d.nodes[0].id, { x: cxm, y: cym });
      shape = d.layout === 'cycle' ? 'ellipse' : 'box';
    } else if (d.layout === 'timeline') {
      const y = top + ah * 0.38, r = rng(el.id + ':axis'), isNew = !seen.has('axis');
      seen.add('axis');
      pen(g, lineD(r, pad * 0.6, y, W - pad * 0.6, y, f * 0.3) + ' ' + arrowHeadD(r, W - pad * 0.6, y, 0, f * 0.8), INK, f * 0.14, isNew, 0, 700);
      d.nodes.forEach((nd, i) => {
        const x = pad + (aw / n) * (i + 0.5), key = 'n:' + nd.label, isN = !seen.has(key), delay = isN ? 250 + i * 120 : 0, c = MARK[i % MARK.length];
        seen.add(key);
        const rr = rng(el.id + key);
        pen(g, lineD(rr, x, y - f * 0.5, x, y + f * 0.5, 0.4), INK, f * 0.14, isN, delay, 200);
        pen(g, ellipseD(rr, x, y, f * 0.35, f * 0.35, 0.08), c, f * 0.16, isN, delay + 100, 260);
        if (nd.note) write(g, nd.note, x, y - f * 1.5, f * 1.35, { fill: c, 'font-weight': 700 }, isN, delay + 150);
        write(g, wrap(nd.label, (aw / n) * 0.9, f * 1.0, 3), x, y + f * 2.4, f * 1.0, { 'font-weight': 700 }, isN, delay + 300);
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
      } else {
        bw = Math.min(f * 10, (aw / cols) * 0.74);
        const byCol = [];
        d.nodes.forEach((nd, i) => (byCol[L[i]] = byCol[L[i]] || []).push(nd));
        byCol.forEach((list, c) => list.forEach((nd, k) => pos.set(nd.id, { x: pad + (aw / cols) * (c + 0.5), y: top + (ah / list.length) * (k + 0.5) })));
        bh = Math.min(bh, (ah / Math.max(...byCol.map(l => l.length))) * 0.78);
      }
    }

    let k = 0;
    const delayOf = new Map();
    d.nodes.forEach(nd => { if (!seen.has('n:' + nd.label)) delayOf.set(nd.id, 180 * k++); });
    const eg = S('g', {}, g);
    for (const e of d.edges) {
      const a = pos.get(e.from), b = pos.get(e.to);
      if (!a || !b) continue;
      const key = 'e:' + e.from + '>' + e.to, isNew = !seen.has(key);
      seen.add(key);
      arrow(eg, el.id + key, a, b, bw, bh, f, isNew, (delayOf.get(e.to) || 0) + 420, d.layout === 'cycle' ? f * 1.6 : 0, d.layout !== 'hub', e.label);
    }
    d.nodes.forEach((nd, i) => {
      const p = pos.get(nd.id);
      if (!p) return;
      const key = 'n:' + nd.label, isNew = !seen.has(key);
      seen.add(key);
      const hub = d.layout === 'hub' && i === 0;
      node(g, el.id + key, nd, i, p, hub ? bw * 1.15 : bw, hub ? bh * 1.15 : bh, f, isNew, delayOf.get(nd.id) || 0, hub ? 'ellipse' : shape);
    });
  }

  // ---------- charts ----------
  function renderChart(svg, el, c, W, H, seen, f) {
    const pad = f * 1.8, pts = c.points;
    const unitSub = c.unit && !['%', '$', '€', '£'].includes(c.unit.trim()) ? c.unit : null;
    const tNew = !seen.has('title:' + c.title);
    seen.add('title:' + c.title);
    const single = c.kind === 'stat' && pts.length === 1 || pts.length === 1;
    const top = title(svg, rng(el.id + ':title'), c.title, single ? null : unitSub, pad, f, tNew, MARK[0]);
    const aw = W - 2 * pad, ah = H - top - pad * 0.7;
    const g = S('g', {}, svg);
    const fresh = p => { const key = 'p:' + p.label + '=' + p.value, n = !seen.has(key); seen.add(key); return n; };
    const defs = S('defs', {}, svg);
    let clipN = 0;
    const clip = d => { const id = `ck${el.id}-${++clipN}-${Math.round(performance.now())}`; S('path', { d }, S('clipPath', { id }, defs)); return `url(#${id})`; };

    if (c.kind === 'stat' || pts.length === 1) {
      const cx = pad + aw / 2, cy = top + ah * 0.45;
      if (pts.length >= 2) {
        const a = pts[0], b = pts[pts.length - 1], big = Math.min(f * 4.6, aw / 7);
        const isN = fresh(a) | fresh(b), r = rng(el.id + ':stat');
        write(g, fmt(a.value, c.unit), cx - aw * 0.27, cy, big * 0.75, { fill: INK_SOFT, 'font-weight': 700 }, isN, 0);
        write(g, a.label, cx - aw * 0.27, cy + big * 0.8, f * 1.0, { fill: INK_SOFT }, isN, 200);
        pen(g, `M${f1(cx - aw * 0.12)},${f1(cy)} Q${f1(cx)},${f1(cy - f * 1.6)} ${f1(cx + aw * 0.08)},${f1(cy)} ` + arrowHeadD(r, cx + aw * 0.08, cy, 0.5, f * 0.9), INK, f * 0.16, isN, 450, 450);
        write(g, fmt(b.value, c.unit), cx + aw * 0.25, cy, big, { 'font-weight': 700 }, isN, 800);
        write(g, b.label, cx + aw * 0.25, cy + big * 0.8, f * 1.0, { fill: INK_SOFT }, isN, 950);
        if (a.value) {
          const ch = ((b.value - a.value) / Math.abs(a.value)) * 100;
          const s = (ch >= 0 ? '+' : '') + (Math.abs(ch) >= 100 ? Math.round(ch) : ch.toFixed(1).replace(/\.0$/, '')) + '%';
          const col = ch >= 0 ? MARK[3] : MARK[0], by = cy + big * 1.55;
          write(g, s, cx, by, f * 1.5, { fill: col, 'font-weight': 700 }, isN, 1200);
          pen(g, ellipseD(r, cx, by, s.length * f * 0.5, f * 1.15, 0.05), col, f * 0.16, isN, 1350, 550);
        }
        return;
      }
      const p = pts[0], big = Math.min(f * 6.5, aw / 4.5), r = rng(el.id + ':stat1'), isN = fresh(p);
      const s = fmt(p.value, c.unit);
      const unitWord = unitSub ? String(c.unit).trim() : '', uw = unitWord.length * f * 1.7 * 0.5;
      const nx = cx - (unitWord ? (uw + f * 0.6) / 2 : 0);          // number + unit centred as one group
      write(g, s, nx, cy, big, { 'font-weight': 700 }, isN, 0);
      if (unitWord) write(g, unitWord, nx + s.length * big * 0.34 + f * 0.9, cy + big * 0.18, f * 1.7, { 'text-anchor': 'start', fill: INK_SOFT, 'font-weight': 700 }, isN, 250);
      const w = Math.max(big * 1.1, s.length * big * 0.5 + (unitWord ? uw + f * 0.6 : 0));
      pen(g, lineD(r, cx - w / 2, cy + big * 0.55, cx + w / 2, cy + big * 0.5, f * 0.5) + ' ' + lineD(r, cx - w / 2 + f, cy + big * 0.66, cx + w / 2 - f * 0.5, cy + big * 0.62, f * 0.5), MARK[0], f * 0.22, isN, 500, 500);
      void nx;
      write(g, wrap(p.label, aw * 0.8, f * 1.2, 2), cx, cy + big * 1.0, f * 1.2, { fill: INK_SOFT }, isN, 700);
      return;
    }

    if (c.kind === 'pie') {
      const total = pts.reduce((s, p) => s + Math.max(0, p.value), 0) || 1;
      const R = Math.min(ah * 0.42, aw * 0.26), cx = pad + aw * 0.3, cy = top + ah / 2;
      let acc = -Math.PI / 2, k = 0;
      const labelAt = [];
      pts.forEach((p, i) => {
        const share = Math.max(0, p.value) / total, a0 = acc, a1 = acc + share * Math.PI * 2, col = MARK[i % MARK.length];
        acc = a1;
        const r = rng(el.id + ':w:' + p.label), isN = fresh(p), delay = isN ? 200 * k++ : 0;
        const arc = [];
        const steps = Math.max(3, Math.ceil(share * 24));
        for (let s = 0; s <= steps; s++) { const t = a0 + ((a1 - a0) * s) / steps, rr = R * (1 + J(r, 0.02)); arc.push([cx + rr * Math.cos(t), cy + rr * Math.sin(t)]); }
        const wedge = `M${cx},${cy} L${f1(arc[0][0])},${f1(arc[0][1])} ` + smooth(arc).replace(/^M[^C]*/, '') + ' Z';
        const wash = S('path', { d: wedge, fill: col, opacity: 0.18 }, g);
        const hatch = pen(g, hatchD(r, cx - R, cy - R, 2 * R, 2 * R, f * 0.55 + i * 1.5, f * 0.2), col, f * 0.1, isN, delay + 250, 700, { 'clip-path': clip(wedge), opacity: 0.75 });
        void hatch;
        pen(g, lineD(r, cx, cy, arc[0][0], arc[0][1], f * 0.2) + ' ' + smooth(arc), INK, f * 0.13, isN, delay, 600);
        if (isN) anim(wash, [{ opacity: 0 }, { opacity: 0.18 }], delay + 200, 500);
        // label outside the slice with a little leader line
        const mid = (a0 + a1) / 2, lx = cx + (R + f * 2.2) * Math.cos(mid), ly = cy + (R + f * 1.6) * Math.sin(mid);
        pen(g, lineD(r, cx + R * 0.92 * Math.cos(mid), cy + R * 0.92 * Math.sin(mid), cx + (R + f * 0.9) * Math.cos(mid), cy + (R + f * 0.7) * Math.sin(mid), 0.5), INK_SOFT, f * 0.08, isN, delay + 500, 200);
        const pct = c.unit && c.unit.trim() === '%' ? fmt(p.value, '%') : Math.round(share * 100) + '%';
        const anchor = Math.cos(mid) >= 0 ? 'start' : 'end';
        write(g, [p.label, pct], lx, ly, f * 1.05, { 'text-anchor': anchor, 'font-weight': 700, fill: INK }, isN, delay + 550);
        labelAt[i] = { x: lx, y: ly, anchor };
      });
      // circle the biggest share's label, like a presenter marking the headline number
      const li = pts.indexOf(pts.reduce((a, b) => (b.value > a.value ? b : a)));
      const lp = labelAt[li], rk = rng(el.id + ':key:' + pts[li].label);
      if (lp) {
        const isNk = !seen.has('key:' + pts[li].label + pts[li].value);
        seen.add('key:' + pts[li].label + pts[li].value);
        const w = Math.max(pts[li].label.length, 4) * f * 0.55 + f * 1.2, cxk = lp.anchor === 'start' ? lp.x + w / 2 - f * 0.4 : lp.x - w / 2 + f * 0.4;
        pen(g, ellipseD(rk, cxk, lp.y, w * 0.62, f * 1.75, 0.05), MARK[li % MARK.length], f * 0.16, isNk, 1100, 600, { opacity: 0.9 });
      }
      return;
    }

    // bar & line: a hand-drawn baseline, labels under it
    const max = Math.max(...pts.map(p => p.value), 0) || 1;
    const x0 = pad, x1 = W - pad, y1 = H - pad - f * 1.9, y0 = top + f * 2.2;
    const Y = v => y1 - (Math.max(0, v) / max) * (y1 - y0);
    const slot = (x1 - x0) / pts.length, X = i => x0 + slot * (i + 0.5);
    const rb = rng(el.id + ':base');
    pen(g, lineD(rb, x0 - f * 0.4, y1, x1 + f * 0.4, y1 + J(rb, 1), f * 0.4), INK, f * 0.15, !seen.has('base'), 0, 500);
    seen.add('base');
    pts.forEach((p, i) => { const key = 'x:' + p.label, isN = !seen.has(key); seen.add(key); write(g, wrap(p.label, slot * 0.95, f * 1.0, 2), X(i), y1 + f * 1.3, f * 1.0, { fill: INK_SOFT }, isN, 150 + i * 120); });

    if (c.kind === 'line') {
      const r = rng(el.id + ':line'), newPts = pts.map(fresh);
      let d = '';
      for (let i = 0; i < pts.length - 1; i++) d += ' ' + lineD(r, X(i), Y(pts[i].value), X(i + 1), Y(pts[i + 1].value), f * 0.35);
      pen(g, d.trim(), MARK[1], f * 0.22, newPts.some(Boolean), 200, 300 * pts.length);
      pts.forEach((p, i) => {
        const rr = rng(el.id + ':pt:' + p.label);
        pen(g, ellipseD(rr, X(i), Y(p.value), f * 0.38, f * 0.38, 0.1), MARK[1], f * 0.18, newPts[i], 200 + i * 300, 250);
        write(g, fmt(p.value, c.unit), X(i), Y(p.value) - f * 1.3, f * 1.2, { 'font-weight': 700 }, newPts[i], 350 + i * 300);
      });
      return;
    }

    const bw = Math.min(slot * 0.56, f * 6.5);
    let k = 0;
    pts.forEach((p, i) => {
      const col = MARK[i % MARK.length], r = rng(el.id + ':bar:' + p.label), isN = fresh(p), delay = isN ? 220 * k++ : 0;
      const x = X(i) - bw / 2, y = Y(p.value), h = Math.max(2, y1 - y);
      const wash = S('rect', { x: x + f * 0.25, y: y + f * 0.2, width: bw, height: Math.max(1, h - f * 0.2), fill: col, opacity: 0.2, transform: `rotate(${f1(J(r, 0.8))} ${X(i)} ${y1})` }, g);
      const box = `M${x},${y1} L${x},${y} L${x + bw},${y} L${x + bw},${y1} Z`;
      pen(g, hatchD(r, x, y, bw, h, f * 0.5, f * 0.2), col, f * 0.11, isN, delay + 350, Math.min(900, 300 + h), { 'clip-path': clip(box), opacity: 0.8 });
      pen(g, lineD(r, x, y1, x, y, f * 0.25) + ' ' + lineD(r, x, y, x + bw, y, f * 0.25) + ' ' + lineD(r, x + bw, y, x + bw, y1, f * 0.25), INK, f * 0.13, isN, delay, 550);
      if (isN) anim(wash, [{ opacity: 0 }, { opacity: 0.2 }], delay + 300, 500);
      write(g, fmt(p.value, c.unit), X(i), y - f * 1.1, f * 1.35, { 'font-weight': 700 }, isN, delay + 500);
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
    render(svg, el, W, H, seen, full) {
      svg.innerHTML = '';
      svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
      const f = Math.max(12, Math.min(W, H * 1.6) * (full ? 0.03 : 0.026));
      if (el.diagram) renderDiagram(svg, el, el.diagram, W, H, seen, f);
      else if (el.chart) renderChart(svg, el, el.chart, W, H, seen, f);
    },
    circleAround, arrowBetween, rng, fmt,
  };
})();
