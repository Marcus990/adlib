// Text measurement and fitting for the diagram / chart renderers. Widths are MEASURED with the font the SVG draws
// with (canvas measureText), never guessed from the character count: a handwriting font is wider than any guess, and
// a guess is how text ended up outside its box.
(function () {
  const ctx = document.createElement('canvas').getContext('2d');
  const memo = new Map();

  function width(text, size, weight, family) {
    const key = weight + '|' + size.toFixed(2) + '|' + family + '|' + text;
    let w = memo.get(key);
    if (w === undefined) {
      ctx.font = `${weight} ${size}px ${family}`;
      w = ctx.measureText(text).width;
      if (memo.size > 5000) memo.clear();
      memo.set(key, w);
    }
    return w;
  }

  // Cut `line` (plus an ellipsis) until it fits.
  function ellipsize(line, maxW, size, weight, family) {
    if (width(line, size, weight, family) <= maxW) return line;
    let s = line.replace(/[\s.…]+$/, '');
    while (s.length > 1 && width(s + '…', size, weight, family) > maxW) s = s.slice(0, -1).replace(/[\s.…]+$/, '');
    return s + '…';
  }

  // Greedy word wrap by measured width. A single word wider than the box is broken between letters.
  function wrap(text, maxW, size, weight, family) {
    const words = String(text).split(/\s+/).filter(Boolean), lines = [];
    let cur = '';
    const push = w => {
      if (width(w, size, weight, family) <= maxW) { cur = w; return; }
      lines.broke = true;                               // a word wider than the box had to be split between letters
      let piece = '';
      for (const ch of w) {
        if (piece && width(piece + ch, size, weight, family) > maxW) { lines.push(piece); piece = ch; } else piece += ch;
      }
      cur = piece;
    };
    for (const w of words) {
      if (!cur) push(w);
      else if (width(cur + ' ' + w, size, weight, family) <= maxW) cur += ' ' + w;
      else { lines.push(cur); cur = ''; push(w); }
    }
    if (cur) lines.push(cur);
    return lines;
  }

  // Largest size in [min, size] at which `text` fits maxW x maxH in at most maxLines lines. If even `min` does not fit,
  // it is cut to maxLines and ellipsized: the text stays inside the box.
  //   o: { size, min, weight = 400, family, maxLines = 2, lh = 1.14 }
  function fit(text, maxW, maxH, o) {
    const weight = o.weight || 400, family = o.family, maxLines = o.maxLines || 2, lhk = o.lh || 1.14;
    const min = Math.min(o.min || o.size * 0.6, o.size);
    let size = o.size, lines = [];
    for (let guard = 0; guard < 40; guard++) {
      lines = wrap(text, maxW, size, weight, family);
      const w = Math.max(0, ...lines.map(l => width(l, size, weight, family)));
      // a split word is a failure to fit: shrink first, and split only when even the smallest size cannot hold the word
      if (!lines.broke && lines.length <= maxLines && lines.length * size * lhk <= maxH + 0.5 && w <= maxW + 0.5) break;
      if (size <= min + 0.01) {
        lines = lines.slice(0, maxLines);
        if (lines.length) lines[lines.length - 1] = lines[lines.length - 1] + (wrap(text, maxW, size, weight, family).length > maxLines ? ' …' : '');
        lines = lines.map(l => ellipsize(l, maxW, size, weight, family));
        break;
      }
      size = Math.max(min, size * 0.93);
    }
    const w = Math.max(0, ...lines.map(l => width(l, size, weight, family)));
    return { lines, size, lh: size * lhk, w, h: lines.length * size * lhk, broke: !!lines.broke };
  }

  // Width of the longest word: the narrowest a box of this text can be without splitting a word.
  function longestWord(text, size, weight, family) {
    return Math.max(0, ...String(text).split(/\s+/).filter(Boolean).map(w => width(w, size, weight, family)));
  }

  window.TextFit = { width, wrap, fit, ellipsize, longestWord };
})();
