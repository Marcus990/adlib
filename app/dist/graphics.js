// Live diagrams and charts for canvas tiles (CANVAS.md). Pure presentation: Rust owns the data.
// Gfx.render(svg, element, W, H, seen, full) draws element.diagram / element.chart into `svg` sized
// W×H px. `seen` (a Set kept per tile) remembers what was already on screen, so only new nodes,
// edges, bars and points animate in when the talk extends a graphic.
(function () {
  const NS = 'http://www.w3.org/2000/svg';
  const PAL = ['#5eead4', '#fbbf24', '#a78bfa', '#f472b6', '#60a5fa', '#34d399', '#fb923c', '#f87171'];
  const TEXT = '#f8fafc', MUTED = '#94a3b8', LINE = 'rgba(226,232,240,0.55)';

  function S(tag, attrs, parent) {
    const el = document.createElementNS(NS, tag);
    for (const k in attrs) if (attrs[k] !== undefined && attrs[k] !== null) el.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(el);
    return el;
  }

  // Approximate wrapping (SVG text doesn't wrap): ~0.56 em per character.
  function wrap(text, maxW, size, maxLines) {
    const per = Math.max(4, Math.floor(maxW / (size * 0.56)));
    const words = String(text).split(/\s+/).filter(Boolean);
    const lines = [];
    let cur = '';
    for (const w of words) {
      if (!cur) cur = w;
      else if ((cur + ' ' + w).length <= per) cur += ' ' + w;
      else { lines.push(cur); cur = w; }
    }
    if (cur) lines.push(cur);
    if (lines.length > maxLines) {
      const keep = lines.slice(0, maxLines);
      keep[maxLines - 1] = keep[maxLines - 1].slice(0, per - 1) + '…';
      return keep;
    }
    return lines.map(l => (l.length > per ? l.slice(0, per - 1) + '…' : l));
  }

  function textBlock(parent, lines, x, y, size, attrs) {
    const t = S('text', Object.assign({ x, y, 'font-size': size, fill: TEXT, 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, attrs), parent);
    const lh = size * 1.18;
    lines.forEach((l, i) => S('tspan', { x, dy: i === 0 ? -((lines.length - 1) * lh) / 2 : lh }, t).textContent = l);
    return t;
  }

  function fmt(v, unit) {
    const u = (unit || '').trim();
    const a = Math.abs(v);
    const trim = x => x.toFixed(1).replace(/\.0$/, '');
    let s;
    if (u === '%') s = trim(v) + '%';
    else if (a >= 1e9) s = trim(v / 1e9) + 'B';
    else if (a >= 1e6) s = trim(v / 1e6) + 'M';
    else if (a >= 1e3) s = trim(v / 1e3) + 'K';
    else s = Number.isInteger(v) ? String(v) : trim(v);
    if (u === '$' || u === '€' || u === '£') s = u + s;
    return s;
  }

  // WAAPI keeps SVG `transform` attributes intact (CSS transforms would override them).
  function anim(el, frames, delay, dur) {
    try { el.animate(frames, { duration: dur || 600, delay: delay || 0, easing: 'cubic-bezier(.2,.8,.2,1)', fill: 'both' }); } catch (_) {}
  }
  function drawIn(path, delay, dur) {
    let len = 0;
    try { len = path.getTotalLength(); } catch (_) {}
    if (!len) return;
    path.style.strokeDasharray = len;
    anim(path, [{ strokeDashoffset: len }, { strokeDashoffset: 0 }], delay, dur || 650);
  }

  function frame(svg, W, H, title, subtitle, f) {
    svg.innerHTML = '';
    svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    const defs = S('defs', {}, svg);
    const m = S('marker', { id: 'ah' + (++frame.n), viewBox: '0 0 10 10', refX: 8.5, refY: 5, markerWidth: 6, markerHeight: 6, orient: 'auto-start-reverse' }, defs);
    S('path', { d: 'M0,0 L10,5 L0,10 z', fill: 'rgba(226,232,240,0.85)' }, m);
    const pad = f * 1.6;
    let top = pad;
    if (title) {
      S('text', { x: pad, y: pad + f * 0.9, 'font-size': f * 1.3, 'font-weight': 700, fill: TEXT, 'letter-spacing': '-0.01em' }, svg).textContent = title;
      top += f * 2.1;
    }
    if (subtitle) {
      S('text', { x: pad, y: top + f * 0.3, 'font-size': f * 0.8, fill: MUTED }, svg).textContent = subtitle;
      top += f * 1.4;
    }
    return { defs, marker: `url(#${m.id})`, pad, top };
  }
  frame.n = 0;

  // ---------------- diagrams ----------------

  function layers(d) {
    const idx = new Map(d.nodes.map((n, i) => [n.id, i]));
    const L = d.nodes.map(() => 0);
    for (let k = 0; k < d.nodes.length; k++) {         // longest path from sources (cycle-safe: bounded)
      let changed = false;
      for (const e of d.edges) {
        const a = idx.get(e.from), b = idx.get(e.to);
        if (a === undefined || b === undefined) continue;
        if (L[b] < L[a] + 1 && L[a] + 1 < d.nodes.length) { L[b] = L[a] + 1; changed = true; }
      }
      if (!changed) break;
    }
    return L;
  }

  function nodeBox(g, n, i, x, y, bw, bh, f, isNew, delay, hub) {
    const outer = S('g', { transform: `translate(${x},${y})` }, g);
    const inner = S('g', {}, outer);
    const c = PAL[i % PAL.length];
    S('rect', { x: -bw / 2, y: -bh / 2, width: bw, height: bh, rx: f * 0.7,
      fill: hub ? c : 'rgba(255,255,255,0.06)', 'fill-opacity': hub ? 0.22 : 1, stroke: c, 'stroke-width': Math.max(1.5, f * 0.1) }, inner);
    let cy = 0;
    const hasIcon = !!n.icon, hasNote = !!n.note;
    const size = f * (hub ? 1.1 : 0.95);
    const lines = wrap(n.label, bw - f * 1.2, size, 2);
    const blockH = (hasIcon ? f * 1.7 : 0) + lines.length * size * 1.18 + (hasNote ? f * 1.1 : 0);
    cy = -blockH / 2;
    if (hasIcon) { textBlock(inner, [n.icon], 0, cy + f * 0.75, f * 1.35); cy += f * 1.7; }
    textBlock(inner, lines, 0, cy + (lines.length * size * 1.18) / 2, size, { 'font-weight': 600 });
    cy += lines.length * size * 1.18;
    if (hasNote) textBlock(inner, [n.note], 0, cy + f * 0.6, f * 0.72, { fill: c, 'font-weight': 600 });
    if (isNew) anim(inner, [{ opacity: 0, transform: 'scale(0.82)' }, { opacity: 1, transform: 'scale(1)' }], delay, 520);
    return outer;
  }

  // Point on the box border facing (tx, ty).
  function port(p, t, bw, bh) {
    const dx = t.x - p.x, dy = t.y - p.y;
    if (Math.abs(dx) * bh > Math.abs(dy) * bw) return { x: p.x + Math.sign(dx) * bw / 2, y: p.y + (dy * bw) / 2 / Math.abs(dx || 1) * (Math.abs(dx) ? 1 : 0) };
    return { x: p.x + (dx * bh) / 2 / Math.abs(dy || 1) * (Math.abs(dy) ? 1 : 0), y: p.y + Math.sign(dy) * bh / 2 };
  }

  function edgePath(g, a, b, bw, bh, f, marker, label, isNew, delay, bow, arrow) {
    const p = port(a, b, bw, bh), q = port(b, a, bw, bh);
    const mx = (p.x + q.x) / 2, my = (p.y + q.y) / 2;
    const nx = -(q.y - p.y), ny = q.x - p.x, nl = Math.hypot(nx, ny) || 1;
    const cx = mx + (nx / nl) * (bow || 0), cy = my + (ny / nl) * (bow || 0);
    const path = S('path', { d: `M${p.x},${p.y} Q${cx},${cy} ${q.x},${q.y}`, fill: 'none', stroke: LINE, 'stroke-width': Math.max(1.5, f * 0.1),
      'stroke-linecap': 'round', 'marker-end': arrow === false ? null : marker }, g);
    if (isNew) drawIn(path, delay, 600);
    if (label) {
      const lx = (p.x + 2 * cx + q.x) / 4, ly = (p.y + 2 * cy + q.y) / 4;
      const lg = S('g', {}, g);
      const w = label.length * f * 0.72 * 0.56 + f * 1.1;
      S('rect', { x: lx - w / 2, y: ly - f * 0.65, width: w, height: f * 1.3, rx: f * 0.65, fill: '#0b0f17', stroke: 'rgba(255,255,255,0.12)' }, lg);
      textBlock(lg, [label], lx, ly, f * 0.72, { fill: MUTED, 'font-weight': 600 });
      if (isNew) anim(lg, [{ opacity: 0 }, { opacity: 1 }], delay + 350, 400);
    }
  }

  function renderDiagram(svg, d, W, H, seen, f) {
    const { marker, pad, top } = frame(svg, W, H, d.title, null, f);
    const n = d.nodes.length;
    const aw = W - 2 * pad, ah = H - top - pad;
    const g = S('g', {}, svg);
    const pos = new Map();
    let bw, bh = f * (d.nodes.some(x => x.icon) ? 4.6 : 3.4) + (d.nodes.some(x => x.note) ? f * 1.1 : 0);
    const cxm = pad + aw / 2, cym = top + ah / 2;

    if (d.layout === 'cycle' || d.layout === 'hub') {
      const ring = d.layout === 'hub' ? d.nodes.slice(1) : d.nodes;
      bw = Math.min(f * 9, aw / Math.max(3, Math.ceil(ring.length / 2) + 1.2));
      bh = Math.min(bh, ah / 3.2);
      const rx = Math.max(bw * 0.8, aw / 2 - bw / 2 - f * 0.4), ry = Math.max(bh * 0.8, ah / 2 - bh / 2 - f * 0.2);
      ring.forEach((nd, i) => {
        const a = -Math.PI / 2 + (2 * Math.PI * i) / Math.max(1, ring.length);
        pos.set(nd.id, { x: cxm + rx * Math.cos(a), y: cym + ry * Math.sin(a) });
      });
      if (d.layout === 'hub') pos.set(d.nodes[0].id, { x: cxm, y: cym });
      else S('text', { x: cxm, y: cym, 'font-size': f * 4, fill: 'rgba(255,255,255,0.07)', 'text-anchor': 'middle', 'dominant-baseline': 'central' }, g).textContent = '↻';
    } else if (d.layout === 'timeline') {
      // Axis across the upper third: date above each dot, the event in a card hanging below.
      bw = Math.min(f * 9, (aw / n) * 0.88);
      bh = Math.min(f * (d.nodes.some(x => x.icon) ? 4.4 : 3.2), ah * 0.5);
      const y = top + ah * 0.34;
      const axis = S('path', { d: `M${pad},${y} L${W - pad},${y}`, stroke: 'rgba(255,255,255,0.2)', 'stroke-width': f * 0.12, 'stroke-linecap': 'round' }, g);
      if (!seen.has('axis')) { drawIn(axis, 0, 700); seen.add('axis'); }
      d.nodes.forEach((nd, i) => pos.set(nd.id, { x: pad + (aw / n) * (i + 0.5), y: y + f * 1.5 + bh / 2, axis: y }));
    } else {
      // flow: columns by layer; a long simple chain snakes onto two rows.
      const L = layers(d);
      const cols = Math.max(...L) + 1;
      const chain = cols === n && n > 4 && aw / n < f * 8;
      if (chain) {
        const perRow = Math.ceil(n / 2);
        bw = Math.min(f * 10, (aw / perRow) * 0.78);
        d.nodes.forEach((nd, i) => {
          const row = i < perRow ? 0 : 1, k = row ? n - 1 - i : i;       // row 2 runs right → left
          const col = row ? k : k;
          const xSlot = row ? perRow - 1 - (i - perRow) : i;
          pos.set(nd.id, { x: pad + (aw / perRow) * (xSlot + 0.5), y: top + (ah / 2) * (row + 0.5) });
          void col;
        });
      } else {
        bw = Math.min(f * 10, (aw / cols) * 0.76);
        const byCol = [];
        d.nodes.forEach((nd, i) => (byCol[L[i]] = byCol[L[i]] || []).push(nd));
        byCol.forEach((list, c) => list.forEach((nd, r) => pos.set(nd.id, { x: pad + (aw / cols) * (c + 0.5), y: top + (ah / list.length) * (r + 0.5) })));
        bh = Math.min(bh, (ah / Math.max(...byCol.map(l => l.length))) * 0.8);
      }
    }

    // Edges under nodes. A new edge draws after its target node has popped in.
    const order = new Map(d.nodes.map((nd, i) => [nd.id, i]));
    let newIdx = 0;
    const nodeDelay = new Map();
    d.nodes.forEach(nd => { if (!seen.has('n:' + nd.label)) nodeDelay.set(nd.id, 140 * newIdx++); });
    const eg = S('g', {}, g);
    for (const e of (d.layout === 'timeline' ? [] : d.edges)) {
      const a = pos.get(e.from), b = pos.get(e.to);
      if (!a || !b) continue;
      const key = 'e:' + e.from + '>' + e.to;
      const isNew = !seen.has(key);
      seen.add(key);
      const bow = d.layout === 'cycle' ? f * 1.2 : 0;
      const delay = (nodeDelay.get(e.to) || 0) + 260;
      edgePath(eg, a, b, bw, bh, f, marker, e.label, isNew, delay, bow, d.layout !== 'hub');
      void order;
    }
    d.nodes.forEach((nd, i) => {
      const p = pos.get(nd.id);
      if (!p) return;
      const isNew = !seen.has('n:' + nd.label);
      seen.add('n:' + nd.label);
      const delay = nodeDelay.get(nd.id) || 0;
      if (d.layout === 'timeline') {
        const c = PAL[i % PAL.length], mark = S('g', {}, g);
        S('line', { x1: p.x, x2: p.x, y1: p.axis, y2: p.y - bh / 2, stroke: c, 'stroke-opacity': 0.5, 'stroke-width': Math.max(1, f * 0.08) }, mark);
        S('circle', { cx: p.x, cy: p.axis, r: f * 0.42, fill: c }, mark);
        if (nd.note) textBlock(mark, [nd.note], p.x, p.axis - f * 1.3, f * 1.15, { fill: c, 'font-weight': 800 });
        if (isNew) anim(mark, [{ opacity: 0 }, { opacity: 1 }], delay, 450);
        nodeBox(g, Object.assign({}, nd, { note: null }), i, p.x, p.y, bw, bh, f, isNew, delay + 120, false);
      } else {
        nodeBox(g, nd, i, p.x, p.y, bw, bh, f, isNew, delay, d.layout === 'hub' && i === 0);
      }
    });
  }

  // ---------------- charts ----------------

  function renderChart(svg, c, W, H, seen, f) {
    const unitSub = c.unit && !['%', '$', '€', '£'].includes(c.unit.trim()) ? c.unit : null;
    const { pad, top } = frame(svg, W, H, c.title, unitSub, f);
    const pts = c.points;
    const aw = W - 2 * pad, ah = H - top - pad;
    const g = S('g', {}, svg);
    const isNew = p => { const k = 'p:' + p.label + '=' + p.value; const n = !seen.has(k); seen.add(k); return n; };

    if (c.kind === 'stat' || pts.length === 1) {
      const cx = pad + aw / 2, cy = top + ah * 0.46;
      if (pts.length >= 2) {
        const a = pts[0], b = pts[pts.length - 1];
        const big = Math.min(f * 4.2, aw / 9);
        const t1 = textBlock(g, [fmt(a.value, c.unit)], cx - aw * 0.25, cy, big * 0.8, { fill: MUTED, 'font-weight': 700 });
        const t2 = textBlock(g, [fmt(b.value, c.unit)], cx + aw * 0.22, cy, big, { 'font-weight': 800 });
        S('text', { x: cx - aw * 0.02, y: cy, 'font-size': big * 0.6, fill: PAL[0], 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, g).textContent = '→';
        textBlock(g, [a.label], cx - aw * 0.25, cy + big * 0.85, f * 0.9, { fill: MUTED });
        textBlock(g, [b.label], cx + aw * 0.22, cy + big * 0.85, f * 0.9, { fill: MUTED });
        if (a.value) {
          const ch = ((b.value - a.value) / Math.abs(a.value)) * 100;
          const s = (ch >= 0 ? '+' : '') + (Math.abs(ch) >= 100 ? Math.round(ch) : ch.toFixed(1).replace(/\.0$/, '')) + '%';
          const col = ch >= 0 ? '#34d399' : '#f87171';
          const bw = s.length * f * 0.62 + f * 1.6;
          const bg = S('g', {}, g);
          S('rect', { x: cx - bw / 2, y: cy + big * 1.45, width: bw, height: f * 1.8, rx: f * 0.9, fill: col, 'fill-opacity': 0.16, stroke: col }, bg);
          textBlock(bg, [s], cx, cy + big * 1.45 + f * 0.9, f * 1.05, { fill: col, 'font-weight': 700 });
          if (isNew(b) | isNew(a)) { anim(t1, [{ opacity: 0 }, { opacity: 1 }], 0, 500); anim(t2, [{ opacity: 0, transform: 'translateY(10px)' }, { opacity: 1, transform: 'none' }], 250, 600); anim(bg, [{ opacity: 0 }, { opacity: 1 }], 650, 500); }
        }
        return;
      }
      const p = pts[0];
      const big = Math.min(f * 6, aw / 5);
      const t = textBlock(g, [fmt(p.value, c.unit)], cx, cy, big, { 'font-weight': 800, fill: TEXT });
      textBlock(g, wrap(p.label, aw * 0.8, f * 1.1, 2), cx, cy + big * 0.85, f * 1.1, { fill: MUTED });
      if (isNew(p)) {
        const t0 = performance.now(), dur = 900;
        const step = now => { const k = Math.min(1, (now - t0) / dur), e = 1 - Math.pow(1 - k, 3);
          t.firstChild.textContent = fmt(p.value * e, c.unit).replace(/\.\d+(?=[KMB%]?$)/, m => (k < 1 ? '' : m)); if (k < 1) requestAnimationFrame(step); };
        requestAnimationFrame(step);
      }
      return;
    }

    if (c.kind === 'pie') {
      const total = pts.reduce((s, p) => s + Math.max(0, p.value), 0) || 1;
      const R = Math.min(ah * 0.42, aw * 0.24), th = R * 0.42;
      const cx = pad + aw * 0.3, cy = top + ah / 2;
      const C = 2 * Math.PI * R;
      let acc = 0, delay = 0;
      S('circle', { cx, cy, r: R, fill: 'none', stroke: 'rgba(255,255,255,0.05)', 'stroke-width': th }, g);
      pts.forEach((p, i) => {
        const share = Math.max(0, p.value) / total;
        const seg = S('circle', { cx, cy, r: R, fill: 'none', stroke: PAL[i % PAL.length], 'stroke-width': th,
          'stroke-dasharray': `${Math.max(0, share * C - 2)} ${C}`, transform: `rotate(${-90 + acc * 360} ${cx} ${cy})` }, g);
        if (isNew(p)) { anim(seg, [{ strokeDashoffset: share * C }, { strokeDashoffset: 0 }], delay, 700); delay += 220; }
        acc += share;
        const ly = top + ah / 2 - ((pts.length - 1) * f * 2.2) / 2 + i * f * 2.2, lx = pad + aw * 0.62;
        S('rect', { x: lx, y: ly - f * 0.45, width: f * 0.9, height: f * 0.9, rx: f * 0.2, fill: PAL[i % PAL.length] }, g);
        S('text', { x: lx + f * 1.5, y: ly, 'font-size': f * 1.0, fill: TEXT, 'dominant-baseline': 'middle', 'font-weight': 600 }, g).textContent = p.label;
        S('text', { x: W - pad, y: ly, 'font-size': f * 1.0, fill: MUTED, 'dominant-baseline': 'middle', 'text-anchor': 'end' }, g).textContent =
          c.unit && c.unit.trim() === '%' ? fmt(p.value, '%') : Math.round(share * 100) + '%';
      });
      const lead = pts.reduce((a, b) => (b.value > a.value ? b : a));
      textBlock(g, [c.unit && c.unit.trim() === '%' ? fmt(lead.value, '%') : Math.round((lead.value / total) * 100) + '%'], cx, cy - f * 0.4, f * 1.7, { 'font-weight': 800 });
      textBlock(g, wrap(lead.label, R * 1.1, f * 0.8, 1), cx, cy + f * 1.1, f * 0.8, { fill: MUTED });
      return;
    }

    // bar / line share axes
    const max = Math.max(...pts.map(p => p.value), 0) || 1, min = Math.min(0, ...pts.map(p => p.value));
    const ax = { x0: pad + f * 0.5, x1: W - pad, y0: top + f * 1.6, y1: H - pad - f * 1.8 };
    const Y = v => ax.y1 - ((v - min) / (max - min)) * (ax.y1 - ax.y0);
    for (let k = 1; k <= 3; k++) {
      const y = ax.y1 - ((ax.y1 - ax.y0) * k) / 3;
      S('line', { x1: ax.x0, x2: ax.x1, y1: y, y2: y, stroke: 'rgba(255,255,255,0.06)', 'stroke-width': 1 }, g);
    }
    S('line', { x1: ax.x0, x2: ax.x1, y1: ax.y1, y2: ax.y1, stroke: 'rgba(255,255,255,0.22)', 'stroke-width': Math.max(1, f * 0.06) }, g);
    const slot = (ax.x1 - ax.x0) / pts.length;
    const X = i => ax.x0 + slot * (i + 0.5);
    pts.forEach((p, i) => textBlock(g, wrap(p.label, slot * 0.95, f * 0.85, 2), X(i), ax.y1 + f * 1.1, f * 0.85, { fill: MUTED }));

    if (c.kind === 'line') {
      const d = pts.map((p, i) => `${i ? 'L' : 'M'}${X(i)},${Y(p.value)}`).join(' ');
      const gid = 'lg' + (++frame.n);
      const lg = S('linearGradient', { id: gid, x1: 0, x2: 0, y1: 0, y2: 1 }, svg.querySelector('defs'));
      S('stop', { offset: '0%', 'stop-color': PAL[4], 'stop-opacity': 0.35 }, lg);
      S('stop', { offset: '100%', 'stop-color': PAL[4], 'stop-opacity': 0 }, lg);
      const area = S('path', { d: `${d} L${X(pts.length - 1)},${ax.y1} L${X(0)},${ax.y1} Z`, fill: `url(#${gid})` }, g);
      const line = S('path', { d, fill: 'none', stroke: PAL[4], 'stroke-width': f * 0.22, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }, g);
      const fresh = pts.map(isNew);
      if (fresh.some(Boolean)) { drawIn(line, 0, 900); anim(area, [{ opacity: 0 }, { opacity: 1 }], 400, 700); }
      pts.forEach((p, i) => {
        const pg = S('g', {}, g);
        S('circle', { cx: X(i), cy: Y(p.value), r: f * 0.38, fill: '#0b0f17', stroke: PAL[4], 'stroke-width': f * 0.16 }, pg);
        textBlock(pg, [fmt(p.value, c.unit)], X(i), Y(p.value) - f * 1.1, f * 0.95, { 'font-weight': 700 });
        if (fresh[i]) anim(pg, [{ opacity: 0 }, { opacity: 1 }], 200 + i * 120, 400);
      });
      return;
    }

    const bw = Math.min(slot * 0.6, f * 6);
    let k = 0;
    pts.forEach((p, i) => {
      const gid = 'bg' + (++frame.n), col = PAL[i % PAL.length];
      const lg = S('linearGradient', { id: gid, x1: 0, x2: 0, y1: 0, y2: 1 }, svg.querySelector('defs'));
      S('stop', { offset: '0%', 'stop-color': col, 'stop-opacity': 1 }, lg);
      S('stop', { offset: '100%', 'stop-color': col, 'stop-opacity': 0.45 }, lg);
      const y = Y(Math.max(p.value, 0)), h = Math.max(2, Math.abs(Y(p.value) - Y(0)));
      const bar = S('rect', { x: X(i) - bw / 2, y, width: bw, height: h, rx: Math.min(f * 0.5, bw / 4), fill: `url(#${gid})` }, g);
      const val = textBlock(g, [fmt(p.value, c.unit)], X(i), y - f * 0.9, f * 1.05, { 'font-weight': 700 });
      if (isNew(p)) {
        const d = 130 * k++;
        anim(bar, [{ transform: `translate(0px, ${h}px) scale(1, 0)` }, { transform: 'translate(0px, 0px) scale(1, 1)' }], d, 750);
        anim(val, [{ opacity: 0 }, { opacity: 1 }], d + 450, 350);
      }
    });
  }

  window.Gfx = {
    render(svg, el, W, H, seen, full) {
      const f = Math.max(11, Math.min(W, H * 1.6) * (full ? 0.026 : 0.024));
      if (el.diagram) renderDiagram(svg, el.diagram, W, H, seen, f);
      else if (el.chart) renderChart(svg, el.chart, W, H, seen, f);
    },
    fmt,
  };
})();
