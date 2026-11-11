/* Hash-seal variants, all on the same idea: bits become strokes on a lattice, stroke and gap
   in a fixed ratio, glyphs in a frame. Browser (window.Seal2) and node. */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.Seal2 = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  function bits(hex, n, from) {
    const out = [];
    for (const ch of hex.replace(/^0x/, '')) {
      const v = parseInt(ch, 16);
      for (let i = 3; i >= 0; i--) out.push((v >> i) & 1);
    }
    const f = from || 0;
    if (out.length < f + n) throw new Error('hash too short: ' + hex);
    return out.slice(f, f + n);
  }

  /* --- glyph generators: bits → unit segments on an n×n lattice --- */

  // binary tree: each interior point strokes up (1) or left (0)
  function bt(b, n) {
    const s = [];
    let k = 0;
    for (let r = 1; r < n; r++)
      for (let c = 1; c < n; c++) {
        const v = b[k++];
        s.push(v ? [c, r, c, r - 1] : [c, r, c - 1, r]);
      }
    return s;
  }
  bt.bits = (n) => (n - 1) * (n - 1);

  // mirrored binary tree: the left half decides, the right half mirrors it (n odd)
  function btMirror(b, n) {
    const s = [];
    const half = (n - 1) / 2; // interior columns 1..half decide
    let k = 0;
    for (let r = 1; r < n; r++)
      for (let c = 1; c <= half; c++) {
        const v = b[k++];
        const m = n - 1 - c + 1; // mirror column: c ↔ n−c
        if (v) {
          s.push([c, r, c, r - 1]);
          s.push([n - c, r, n - c, r - 1]);
        } else {
          s.push([c, r, c - 1, r]);
          s.push([n - c, r, n - c + 1, r]);
        }
      }
    return s;
  }
  btMirror.bits = (n) => (n - 1) * ((n - 1) / 2);

  // stacked folds: a bar on every row; each bit a rung between two bars at one column
  function folds(b, n) {
    const s = [];
    for (let r = 0; r < n; r++) s.push([0, r, n - 1, r]);
    let k = 0;
    for (let r = 0; r < n - 1; r++)
      for (let c = 0; c < n - 1; c++) if (b[k++]) s.push([c + (r % 2 ? 1 : 0), r, c + (r % 2 ? 1 : 0), r + 1]);
    return s;
  }
  folds.bits = (n) => (n - 1) * (n - 1);

  // one folded line: a self-avoiding walk; bit 1 prefers to turn, bit 0 to go straight
  function walk(b, n) {
    const s = [];
    const used = new Set();
    const key = (x, y) => x + ',' + y;
    const dirs = [[1, 0], [0, 1], [-1, 0], [0, -1]]; // E S W N
    let k = 0;
    let steps = 0;
    const total = b.length;
    let starts = 0;
    while (k < total && starts < 3) {
      // first free lattice point, row-major
      let sx = -1, sy = -1;
      outer: for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) if (!used.has(key(x, y))) { sx = x; sy = y; break outer; }
      if (sx < 0) break;
      starts++;
      let x = sx, y = sy, h = 0;
      used.add(key(x, y));
      while (k < total) {
        const v = b[k++];
        const order = v ? [(h + 3) % 4, (h + 1) % 4, h] : [h, (h + 3) % 4, (h + 1) % 4];
        let moved = false;
        for (const d of order) {
          const nx = x + dirs[d][0], ny = y + dirs[d][1];
          if (nx < 0 || ny < 0 || nx >= n || ny >= n || used.has(key(nx, ny))) continue;
          s.push([x, y, nx, ny]);
          used.add(key(nx, ny));
          x = nx; y = ny; h = d; moved = true; steps++;
          break;
        }
        if (!moved) break;
      }
    }
    return s;
  }
  walk.bits = (n) => (n - 1) * (n - 1);

  // brick: a full bar on every row; each bit may knock one unit out of a bar
  function brick(b, n) {
    const s = [];
    let k = 0;
    for (let r = 0; r < n; r++) for (let c = 1; c < n; c++) if (!b[k++]) s.push([c - 1, r, c, r]);
    return s;
  }
  brick.bits = (n) => n * (n - 1);

  // spiral: a rectangular spiral over the lattice (the thunder pattern); the first bit sets the
  // turning, and a segment is cut where two bits agree
  function spiral(b, n) {
    const pts = [];
    let left = 0, right = n - 1, top = 0, bottom = n - 1;
    pts.push([0, 0]);
    while (left <= right && top <= bottom) {
      if (pts.length > 1) pts.push([left, top]);
      for (let c = left + 1; c <= right; c++) pts.push([c, top]);
      for (let r = top + 1; r <= bottom; r++) pts.push([right, r]);
      if (top < bottom) for (let c = right - 1; c >= left; c--) pts.push([c, bottom]);
      if (left < right) for (let r = bottom - 1; r > top; r--) pts.push([left, r]);
      left++; right--; top++; bottom--;
    }
    const m = b.length;
    const s = [];
    for (let i = 0; i + 1 < pts.length; i++) {
      if (b[(i + 1) % m] && b[(i + 6) % m]) continue;
      let [x1, y1] = pts[i], [x2, y2] = pts[i + 1];
      if (x1 === x2 && y1 === y2) continue;
      if (b[0]) { x1 = n - 1 - x1; x2 = n - 1 - x2; }
      s.push([x1, y1, x2, y2]);
    }
    return s;
  }
  spiral.bits = (n) => (n - 1) * (n - 1);

  // stitch: the running-stitch pattern; each row and each column takes one bit for the phase
  // of its dashes, and the dashes weave
  function stitch(b, n) {
    const s = [];
    for (let r = 0; r < n; r++) { const ph = b[r]; for (let c = 0; c < n - 1; c++) if ((c + ph) % 2 === 0) s.push([c, r, c + 1, r]); }
    for (let c = 0; c < n; c++) { const ph = b[n + c]; for (let r = 0; r < n - 1; r++) if ((r + ph) % 2 === 0) s.push([c, r, c, r + 1]); }
    return s;
  }
  stitch.bits = (n) => 2 * n;

  // quadMirror: four-fold symmetry, the top-left quadrant decides (n odd)
  function quadMirror(b, n) {
    const h = (n - 1) / 2, m = n - 1, s = [];
    let k = 0;
    for (let r = 1; r <= h; r++)
      for (let c = 1; c <= h; c++) {
        const v = b[k++];
        if (v) { s.push([c, r, c, r - 1]); s.push([m - c, r, m - c, r - 1]); s.push([c, m - r, c, m - r + 1]); s.push([m - c, m - r, m - c, m - r + 1]); }
        else { s.push([c, r, c - 1, r]); s.push([m - c, r, m - c + 1, r]); s.push([c, m - r, c - 1, m - r]); s.push([m - c, m - r, m - c + 1, m - r]); }
      }
    return s;
  }
  quadMirror.bits = (n) => ((n - 1) / 2) ** 2;

  const GEN = { bt, btMirror, folds, walk, brick, spiral, stitch, quadMirror };

  /* --- the renderer --- */
  function render(hex, size, o) {
    o = Object.assign({
      gen: 'bt', n: 5, cols: 2, rows: 2, ratio: 1, framew: 1, frame: 'single', style: 'zhu',
      ink: '#c2452d', ground: '#15120f', rough: true, roughPx: 1.8, id: 'v', tilt: 0, pad: 1.5,
    }, o || {});
    const gen = GEN[o.gen];
    const per = gen.bits(o.n);
    const count = o.cols * o.rows;
    const b = bits(hex, per * count);
    const u = 1, w = o.ratio, p = u + w;
    const span = (o.n - 1) * p + w;
    const gap = p;
    const wf = o.frame === 'none' || o.style === 'bai' ? 0 : w * o.framew;
    const pad = o.pad * u;
    const W = 2 * wf + 2 * pad + o.cols * span + (o.cols - 1) * gap;
    const H = 2 * wf + 2 * pad + o.rows * span + (o.rows - 1) * gap;
    let d = '';
    for (let gy = 0; gy < o.rows; gy++)
      for (let gx = 0; gx < o.cols; gx++) {
        const i = gy * o.cols + gx;
        const segs = gen(b.slice(i * per, (i + 1) * per), o.n);
        const ox = wf + pad + w / 2 + gx * (span + gap);
        const oy = wf + pad + w / 2 + gy * (span + gap);
        for (const [x1, y1, x2, y2] of segs)
          d += `M${(ox + x1 * p).toFixed(2)} ${(oy + y1 * p).toFixed(2)}L${(ox + x2 * p).toFixed(2)} ${(oy + y2 * p).toFixed(2)}`;
      }
    const scale = size / Math.max(W, H);
    const pxW = Math.round(W * scale), pxH = Math.round(H * scale);
    const pxPerUnit = scale;
    const fid = `ink-${o.id}-${hex.slice(2, 8)}`;
    const useRough = o.rough && size >= 40;
    const filter = useRough
      ? `<filter id="${fid}" x="-6%" y="-6%" width="112%" height="112%"><feTurbulence type="fractalNoise" baseFrequency="${(0.35 / pxPerUnit).toFixed(3)}" numOctaves="2" seed="7" result="n"/><feDisplacementMap in="SourceGraphic" in2="n" scale="${(2 * o.roughPx / pxPerUnit).toFixed(2)}" xChannelSelector="R" yChannelSelector="G"/></filter>`
      : '';
    const fattr = useRough ? ` filter="url(#${fid})"` : '';
    let body = '';
    if (o.style === 'bai') {
      body = `<rect x="0" y="0" width="${W}" height="${H}" rx="${0.4 * u}" fill="${o.ink}"/><path d="${d}" fill="none" stroke="${o.ground}" stroke-width="${w}" stroke-linecap="square"/>`;
    } else {
      let frame = '';
      if (o.frame === 'single') frame = `<rect x="${wf / 2}" y="${wf / 2}" width="${W - wf}" height="${H - wf}" rx="${wf * 0.5}" fill="none" stroke="${o.ink}" stroke-width="${wf}"/>`;
      else if (o.frame === 'worn') {
        // sixteen frame segments; a segment is missing where two consecutive extra bits are 1
        const eb = bits(hex, 17, 64);
        const segs = [];
        const inset = wf / 2;
        const sides = [
          [inset, inset, W - inset, inset], [W - inset, inset, W - inset, H - inset],
          [W - inset, H - inset, inset, H - inset], [inset, H - inset, inset, inset],
        ];
        let i = 0;
        for (const [x1, y1, x2, y2] of sides)
          for (let q = 0; q < 4; q++, i++) {
            if (eb[i] && eb[i + 1]) continue;
            const ax = x1 + ((x2 - x1) * q) / 4, ay = y1 + ((y2 - y1) * q) / 4;
            const bx = x1 + ((x2 - x1) * (q + 1)) / 4, by = y1 + ((y2 - y1) * (q + 1)) / 4;
            segs.push(`M${ax.toFixed(2)} ${ay.toFixed(2)}L${bx.toFixed(2)} ${by.toFixed(2)}`);
          }
        frame = `<path d="${segs.join('')}" fill="none" stroke="${o.ink}" stroke-width="${wf}" stroke-linecap="butt"/>`;
      }
      body = `${frame}<path d="${d}" fill="none" stroke="${o.ink}" stroke-width="${w}" stroke-linecap="square"/>`;
    }
    return `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${W.toFixed(2)} ${H.toFixed(2)}" role="img" aria-label="Hash seal ${hex.slice(0, 10)}"${o.tilt ? ` style="transform: rotate(${o.tilt}deg)"` : ''}><g${fattr}>${body}</g>${filter}</svg>`;
  }

  const VARIANTS = {
    zhu:    { label: 'red-text, 1 : 1', desc: 'the baseline: 2×2 glyphs, stroke = gap, a stroke-wide frame', gen: 'bt' },
    xizhu:  { label: 'fine red-text', desc: 'strokes half the gap, a frame three strokes wide: the Yuan collectors’ style', gen: 'bt', ratio: 0.5, framew: 3, roughPx: 1.2, pad: 2 },
    bai:    { label: 'white-text, 1 : 1', desc: 'the same glyphs cut into a red stone; no frame, the stone is the edge', gen: 'bt', style: 'bai' },
    han:    { label: 'full white-text', desc: 'cuts 1.6 × the red between them: the Han habit, mostly white', gen: 'bt', style: 'bai', ratio: 1.6 },
    one:    { label: 'one character', desc: 'all 64 bits on one 9×9 lattice: a one-word seal', gen: 'bt', n: 9, cols: 1, rows: 1 },
    tall:   { label: 'four in a column', desc: 'the same four glyphs stacked: a tall collector’s seal', gen: 'bt', cols: 1, rows: 4 },
    mirror: { label: 'mirrored glyphs', desc: 'each glyph symmetric, as many seal characters are; 32 bits, not 64', gen: 'btMirror' },
    folds:  { label: 'stacked folds', desc: 'five bars a glyph, each bit a rung between two bars: the nine-fold habit', gen: 'folds' },
    walk:   { label: 'one folded line', desc: 'a glyph is one self-avoiding line; a 1 turns, a 0 goes straight', gen: 'walk' },
    worn:   { label: 'worn edge', desc: 'the baseline with a broken frame from bits 65–80 and a rougher paste', gen: 'bt', frame: 'worn', roughPx: 2.6 },
    fineMirror: { label: 'fine red-text, mirrored', desc: 'the Yuan frame and strokes with symmetric glyphs; 32 bits', gen: 'btMirror', ratio: 0.5, framew: 3, roughPx: 1.2, pad: 2 },
    mirror9: { label: 'nine mirrored glyphs', desc: '3×3 symmetric glyphs, 8 bits each: 72 bits, a nine-word seal', gen: 'btMirror', cols: 3, rows: 3 },
  };
  /* the lattice a variant uses below 40 px, where a 5×5 lattice is finer than a pixel */
  const SMALL = { one: { n: 5 }, mirror: { n: 3 }, fineMirror: { n: 3 }, mirror9: { n: 3 } };
  function small(name) { return Object.assign({ n: 3 }, SMALL[name] || {}); }

  function variant(name, hex, size, o) {
    const v = VARIANTS[name];
    if (!v) throw new Error('no variant ' + name);
    const { label, desc, ...cfg } = v;
    return render(hex, size, Object.assign({}, cfg, { id: name }, o || {}));
  }

  return { bits, render, variant, VARIANTS, GEN, small };
});
