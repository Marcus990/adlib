// Dev-only (loaded when index.html runs in a plain browser, not in Tauri): push sample scenes to preview
// themes without the pipeline. Serve the repo root (`python3 -m http.server`) and open /app/dist/index.html,
// then call e.g. demo.photos(), demo.charts(), demo.diagrams(), demo.board(), demo.full('pie').
(function () {
  const G = 0.02, r = (x, y, w, h) => ({ x, y, w, h });
  function rects(layout, n, focus) {
    const hero = () => { const o = n - 1, sh = (1 - G * (o + 1)) / o; let k = 0; return [...Array(n)].map((_, i) => i === focus ? r(G, G, 0.66 - 1.5 * G, 1 - 2 * G) : r(0.66 + G * 0.5, G + (k++) * (sh + G), 0.34 - 1.5 * G, sh)); };
    const row = () => { const w = (1 - G * (n + 1)) / n; return [...Array(n)].map((_, i) => r(G + i * (w + G), 0.12, w, 0.76)); };
    const grid = () => { const cols = n <= 1 ? 1 : 2, rows = Math.ceil(n / cols), w = (1 - G * (cols + 1)) / cols, h = (1 - G * (rows + 1)) / rows; return [...Array(n)].map((_, i) => r(G + (i % cols) * (w + G), G + Math.floor(i / cols) * (h + G), w, h)); };
    if (n === 1) return [r(0, 0, 1, 1)];
    if (layout === 'compare' || (layout === 'auto' && n === 2)) return row();
    if (layout === 'hero' || (layout === 'auto' && n === 3)) return hero();
    return grid();
  }
  let v = 0;
  function show(els, layout = 'auto', anns = []) {
    const fi = els.findIndex(e => e.focus), rs = rects(layout, els.length, fi < 0 ? els.length - 1 : fi);
    els.forEach((e, i) => { e.rect = rs[i]; e.kind = e.kind || 'image'; e.image_id = e.image_id || ''; e.caption = e.caption || ''; e.url = e.url || ''; });
    window.__scene({ version: ++v, layout, elements: els, annotations: anns, reason: 'preview', chunk_id: 0 });
  }
  const img = (id, cap, focus) => ({ id: 'e-' + id, kind: 'image', image_id: id, caption: cap, url: '/dev-library/' + id + '.jpg', focus: !!focus });
  const chain = (labels, icons) => ({ nodes: labels.map((l, i) => ({ id: 'n' + (i + 1), label: l, icon: icons && icons[i] })), edges: labels.slice(1).map((_, i) => ({ from: 'n' + (i + 1), to: 'n' + (i + 2) })) });
  const G_ = {
    bar: { id: 'c-bar', kind: 'chart', chart: { kind: 'bar', title: 'Club members', unit: 'members', points: [{ label: 'Last year', value: 200 }, { label: 'This year', value: 500 }, { label: 'Next year', value: 1000 }] } },
    pie: { id: 'c-pie', kind: 'chart', chart: { kind: 'pie', title: 'Who they are', unit: '%', points: [{ label: 'Students', value: 50 }, { label: 'Teachers', value: 30 }, { label: 'Parents', value: 20 }] } },
    stat: { id: 'c-stat', kind: 'chart', chart: { kind: 'stat', title: 'Members', points: [{ label: 'Last year', value: 200 }, { label: 'This year', value: 500 }] } },
    stat1: { id: 'c-stat1', kind: 'chart', chart: { kind: 'stat', title: 'Eagle eyesight', unit: 'miles', points: [{ label: 'how far an eagle can spot prey', value: 2 }] } },
    line: { id: 'c-line', kind: 'chart', chart: { kind: 'line', title: 'Revenue', unit: '$', points: [{ label: '2021', value: 1.2e6 }, { label: '2022', value: 2.1e6 }, { label: '2023', value: 3.8e6 }, { label: '2024', value: 5.2e6 }] } },
    flow: { id: 'd-flow', kind: 'diagram', diagram: Object.assign({ layout: 'flow', title: 'How it works', auto_edges: true }, chain(['You speak', 'Speech to text', 'Model decides', 'Picture appears'], ['🎤', '📝', '🧠', '🖼️'])) },
    cycle: { id: 'd-cycle', kind: 'diagram', diagram: Object.assign({ layout: 'cycle', title: 'The loop', auto_edges: true }, (() => { const c = chain(['Listen', 'Decide', 'Show', 'Learn']); c.edges.push({ from: 'n4', to: 'n1' }); return c; })()) },
    hub: { id: 'd-hub', kind: 'diagram', diagram: { layout: 'hub', title: 'Our users', auto_edges: true, nodes: ['Live Slides', 'Students', 'Teachers', 'Parents', 'Speakers'].map((l, i) => ({ id: 'n' + (i + 1), label: l })), edges: [2, 3, 4, 5].map(k => ({ from: 'n1', to: 'n' + k })) } },
    timeline: { id: 'd-tl', kind: 'diagram', diagram: { layout: 'timeline', title: 'Company history', auto_edges: true, nodes: [['Founded in a dorm', '2019'], ['First customer', '2021'], ['Series A', '2023'], ['Global launch', '2025']].map(([l, y], i) => ({ id: 'n' + (i + 1), label: l, note: y })), edges: [] } },
  };
  const clone = o => JSON.parse(JSON.stringify(o));
  window.demo = {
    show, img, G: G_,
    photos: () => show([img('animals-penguin', 'penguin (animals)'), img('animals-owl', 'owl (animals)', true)], 'compare', [{ id: 'a1', kind: 'highlight', targets: ['e-animals-owl'], label: 'night hunter' }]),
    photo: () => show([img('flowers-sunflower', 'sunflower (flowers)', true)]),
    charts: () => show([clone(G_.bar), clone(G_.pie), clone(G_.stat), clone(G_.line)], 'grid'),
    diagrams: () => show([clone(G_.flow), clone(G_.cycle), clone(G_.hub), clone(G_.timeline)], 'grid'),
    board: () => show([img('animals-penguin', 'penguin (animals)'), clone(G_.bar), clone(G_.pie), Object.assign(clone(G_.flow), { focus: true })], 'grid'),
    full: k => show([Object.assign(clone(G_[k]), { focus: true })]),
  };
})();
