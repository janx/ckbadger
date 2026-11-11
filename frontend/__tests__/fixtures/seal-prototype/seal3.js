/* The hand: the same bits, different strokes. Builds on seal2's generators and layout; the
   unit segments of a glyph are merged into runs (strokes) or chained into paths, then drawn by
   a "hand": square (the knife of Han seals), wire (iron-wire small seal), chop (the Zhejiang
   chopping knife), flow (the Anhui pushing knife), clerical (隶: silkworm head, swallow tail),
   regular (楷: pauses and hooks), running (行: joined, with threads), clay (sealing clay),
   broken (a general's seal cut in haste). Browser (window.Seal3) and node. */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory(require('./seal2.js'));
  else root.Seal3 = factory(root.Seal2);
})(typeof self !== 'undefined' ? self : this, function (S2) {
  const f2 = (v) => (Math.round(v * 100) / 100).toString();

  /* unit segments → horizontal/vertical runs (strokes) in lattice units */
  function runs(segs) {
    const H = new Map(), V = new Map();
    for (let [x1, y1, x2, y2] of segs) {
      if (x1 > x2) [x1, x2] = [x2, x1];
      if (y1 > y2) [y1, y2] = [y2, y1];
      if (y1 === y2) { if (!H.has(y1)) H.set(y1, new Set()); H.get(y1).add(x1); }
      else { if (!V.has(x1)) V.set(x1, new Set()); V.get(x1).add(y1); }
    }
    const out = [];
    const merge = (map, dir) => {
      for (const [k, set] of map) {
        const xs = [...set].sort((a, b) => a - b);
        let s = xs[0], e = xs[0];
        for (let i = 1; i <= xs.length; i++) {
          if (i < xs.length && xs[i] === e + 1) { e = xs[i]; continue; }
          out.push(dir === 'h' ? { dir, y: k, a: s, b: e + 1 } : { dir, x: k, a: s, b: e + 1 });
          if (i < xs.length) { s = xs[i]; e = xs[i]; }
        }
      }
    };
    merge(H, 'h');
    merge(V, 'v');
    return out;
  }

  /* unit segments → polylines (lattice points), straight continuation preferred */
  function paths(segs) {
    const key = (x, y) => x + ',' + y;
    const adj = new Map();
    const edges = [];
    for (const [x1, y1, x2, y2] of segs) {
      const id = edges.length;
      edges.push({ a: [x1, y1], b: [x2, y2], used: false });
      for (const [p, q] of [[[x1, y1], [x2, y2]], [[x2, y2], [x1, y1]]]) {
        const k = key(p[0], p[1]);
        if (!adj.has(k)) adj.set(k, []);
        adj.get(k).push({ id, to: q });
      }
    }
    const deg = (k) => adj.get(k).filter((e) => !edges[e.id].used).length;
    const out = [];
    const starts = [...adj.keys()].sort((a, b) => (adj.get(a).length % 2 === 1 ? 0 : 1) - (adj.get(b).length % 2 === 1 ? 0 : 1));
    for (const s of starts) {
      while (deg(s) > 0) {
        const pts = [s.split(',').map(Number)];
        let cur = s, dir = null;
        for (;;) {
          const opts = adj.get(cur).filter((e) => !edges[e.id].used);
          if (!opts.length) break;
          let pick = opts[0];
          if (dir) {
            const straight = opts.find((e) => e.to[0] - pts[pts.length - 1][0] === dir[0] && e.to[1] - pts[pts.length - 1][1] === dir[1]);
            if (straight) pick = straight;
          }
          edges[pick.id].used = true;
          const last = pts[pts.length - 1];
          dir = [pick.to[0] - last[0], pick.to[1] - last[1]];
          pts.push(pick.to);
          cur = key(pick.to[0], pick.to[1]);
        }
        if (pts.length > 1) out.push(pts);
      }
    }
    return out;
  }

  function roundedPath(P, r) {
    let d = `M${f2(P[0][0])} ${f2(P[0][1])}`;
    for (let i = 1; i < P.length - 1; i++) {
      const A = P[i - 1], C = P[i], B = P[i + 1];
      const din = [Math.sign(C[0] - A[0]), Math.sign(C[1] - A[1])];
      const dout = [Math.sign(B[0] - C[0]), Math.sign(B[1] - C[1])];
      if (din[0] === dout[0] && din[1] === dout[1]) continue;
      const p1 = [C[0] - din[0] * r, C[1] - din[1] * r];
      const p2 = [C[0] + dout[0] * r, C[1] + dout[1] * r];
      d += `L${f2(p1[0])} ${f2(p1[1])}Q${f2(C[0])} ${f2(C[1])} ${f2(p2[0])} ${f2(p2[1])}`;
    }
    const L = P[P.length - 1];
    d += `L${f2(L[0])} ${f2(L[1])}`;
    return d;
  }

  function smoothPath(P) {
    if (P.length === 2) return `M${f2(P[0][0])} ${f2(P[0][1])}L${f2(P[1][0])} ${f2(P[1][1])}`;
    let d = `M${f2(P[0][0])} ${f2(P[0][1])}`;
    for (let i = 0; i < P.length - 1; i++) {
      const p0 = P[Math.max(0, i - 1)], p1 = P[i], p2 = P[i + 1], p3 = P[Math.min(P.length - 1, i + 2)];
      const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
      const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
      d += `C${f2(c1[0])} ${f2(c1[1])} ${f2(c2[0])} ${f2(c2[1])} ${f2(p2[0])} ${f2(p2[1])}`;
    }
    return d;
  }

  /* polygon helpers for a run given in user units: (X1,Y1)→(X2,Y2) axis-aligned */
  function zigzag(X1, Y1, X2, Y2, w, step, amp, phase) {
    const horiz = Y1 === Y2;
    const L = horiz ? X2 - X1 : Y2 - Y1;
    const n = Math.max(2, Math.round(L / step));
    const pts = [];
    for (let i = 0; i <= n; i++) {
      const t = i / n, off = (i + phase) % 2 ? amp : -amp;
      pts.push(horiz ? [X1 + t * L, Y1 - w / 2 + off] : [X1 - w / 2 + off, Y1 + t * L]);
    }
    for (let i = n; i >= 0; i--) {
      const t = i / n, off = (i + phase + 1) % 2 ? amp : -amp;
      pts.push(horiz ? [X1 + t * L, Y1 + w / 2 + off] : [X1 + w / 2 + off, Y1 + t * L]);
    }
    return `<polygon points="${pts.map((p) => f2(p[0]) + ',' + f2(p[1])).join(' ')}"/>`;
  }
  function lens(X1, Y1, X2, Y2, w) {
    const horiz = Y1 === Y2;
    const e = 0.32 * w, m = 0.66 * w;
    if (horiz) {
      const mx = (X1 + X2) / 2;
      return `<path d="M${f2(X1)} ${f2(Y1 - e)}Q${f2(mx)} ${f2(Y1 - m)} ${f2(X2)} ${f2(Y1 - e)}L${f2(X2)} ${f2(Y1 + e)}Q${f2(mx)} ${f2(Y1 + m)} ${f2(X1)} ${f2(Y1 + e)}Z"/>`;
    }
    const my = (Y1 + Y2) / 2;
    return `<path d="M${f2(X1 - e)} ${f2(Y1)}Q${f2(X1 - m)} ${f2(my)} ${f2(X1 - e)} ${f2(Y2)}L${f2(X1 + e)} ${f2(Y2)}Q${f2(X1 + m)} ${f2(my)} ${f2(X1 + e)} ${f2(Y1)}Z"/>`;
  }
  const rect = (x, y, w, h, extra) => `<rect x="${f2(x)}" y="${f2(y)}" width="${f2(w)}" height="${f2(h)}"${extra || ''}/>`;

  const HANDS = {
    square: { label: 'square knife', desc: 'the baseline: even width, square ends, right angles; the Han seal habit', w: 1 },
    wire: { label: 'iron wire', desc: 'small-seal script: strokes chained, corners rounded, thin and even', w: 0.62 },
    chop: { label: 'chopping knife', desc: 'the Zhejiang school: every edge cut in short chops, serrated', w: 1 },
    flow: { label: 'pushing knife', desc: 'the Anhui school: one push per stroke, fuller in the middle, clean ends', w: 1.05 },
    clerical: { label: 'clerical', desc: '隶: wide and flat; a silkworm head, and one swallow tail per glyph', w: 0.95, aspect: [1.22, 0.82] },
    regular: { label: 'regular', desc: '楷: thin horizontals that pause at the end, firm verticals, one hook per glyph', w: 1 },
    running: { label: 'running', desc: '行: strokes joined where the brush did not lift, threads where it barely did', w: 0.72 },
    clay: { label: 'sealing clay', desc: '封泥: the impression a seal left in clay; soft, swollen, a thick uneven rim', w: 1.3 },
    broken: { label: 'cut in haste', desc: '急就章: a general’s seal cut in the field; strokes break, the frame leans', w: 1 },
  };

  function render(hex, size, o) {
    o = Object.assign({ gen: 'bt', n: 5, cols: 2, rows: 2, ratio: 1, framew: 1, style: 'zhu', hand: 'square',
      ink: '#c2452d', ground: '#15120f', rough: true, roughPx: 1.6, id: 'h', tilt: 0, pad: 1.5 }, o || {});
    const hand = HANDS[o.hand];
    const gen = S2.GEN[o.gen];
    const per = gen.bits(o.n);
    const count = o.cols * o.rows;
    const b = S2.bits(hex, per * count);
    const xb = S2.bits(hex, 64, 64); // extra bits for breaks and jitter
    const u = 1, w0 = o.ratio, p = u + w0;
    const [ax, ay] = hand.aspect || [1, 1];
    const w = w0 * hand.w;
    const spanX = (o.n - 1) * p * ax + w0, spanY = (o.n - 1) * p * ay + w0;
    const gapX = p * ax, gapY = p * ay;
    const wf = o.style === 'bai' || o.frame === 'none' ? 0 : w0 * o.framew * (o.hand === 'clay' ? 2.2 : 1);
    const pad = o.pad * u;
    const W = 2 * wf + 2 * pad + o.cols * spanX + (o.cols - 1) * gapX;
    const H = 2 * wf + 2 * pad + o.rows * spanY + (o.rows - 1) * gapY;
    const scale = size / Math.max(W, H);
    const pxPerUnit = scale;
    const col = o.style === 'bai' ? o.ground : o.ink;
    let shapes = '';
    let xk = 0;
    for (let gy = 0; gy < o.rows; gy++)
      for (let gx = 0; gx < o.cols; gx++) {
        const i = gy * o.cols + gx;
        const segs = gen(b.slice(i * per, (i + 1) * per), o.n);
        const ox = wf + pad + w0 / 2 + gx * (spanX + gapX);
        const oy = wf + pad + w0 / 2 + gy * (spanY + gapY);
        const X = (x) => ox + x * p * ax, Y = (y) => oy + y * p * ay;
        if (o.hand === 'wire' || o.hand === 'running') {
          const P = paths(segs).map((pts) => pts.map(([x, y]) => [X(x), Y(y)]));
          if (o.hand === 'wire') {
            for (const pts of P) shapes += `<path d="${roundedPath(pts, 0.42 * p)}" fill="none" stroke="${col}" stroke-width="${f2(w)}" stroke-linecap="round" stroke-linejoin="round"/>`;
          } else {
            for (let k = 0; k < P.length; k++) {
              shapes += `<path d="${smoothPath(P[k])}" fill="none" stroke="${col}" stroke-width="${f2(w)}" stroke-linecap="round" stroke-linejoin="round"/>`;
              if (k + 1 < P.length) {
                const e = P[k][P[k].length - 1], s = P[k + 1][0];
                shapes += `<path d="M${f2(e[0])} ${f2(e[1])}Q${f2((e[0] + s[0]) / 2 + 0.3)} ${f2((e[1] + s[1]) / 2 - 0.3)} ${f2(s[0])} ${f2(s[1])}" fill="none" stroke="${col}" stroke-width="${f2(w * 0.22)}" stroke-linecap="round" opacity=".8"/>`;
              }
            }
          }
          continue;
        }
        const R = runs(segs);
        let longestH = null, longestV = null;
        for (const r of R) {
          if (r.dir === 'h' && (!longestH || r.b - r.a > longestH.b - longestH.a)) longestH = r;
          if (r.dir === 'v' && (!longestV || r.b - r.a > longestV.b - longestV.a)) longestV = r;
        }
        for (const r of R) {
          const horiz = r.dir === 'h';
          const X1 = horiz ? X(r.a) : X(r.x), X2 = horiz ? X(r.b) : X(r.x);
          const Y1 = horiz ? Y(r.y) : Y(r.a), Y2 = horiz ? Y(r.y) : Y(r.b);
          const len = horiz ? X2 - X1 : Y2 - Y1;
          switch (o.hand) {
            case 'square':
              shapes += `<line x1="${f2(X1)}" y1="${f2(Y1)}" x2="${f2(X2)}" y2="${f2(Y2)}" stroke="${col}" stroke-width="${f2(w)}" stroke-linecap="square"/>`;
              break;
            case 'clay':
              shapes += `<line x1="${f2(X1)}" y1="${f2(Y1)}" x2="${f2(X2)}" y2="${f2(Y2)}" stroke="${col}" stroke-width="${f2(w)}" stroke-linecap="round"/>`;
              break;
            case 'chop':
              shapes += zigzag(X1 - w / 2, Y1 - (horiz ? 0 : w / 2), X2 + (horiz ? w / 2 : 0), Y2 + (horiz ? 0 : w / 2), w, 0.42, 0.09, r.a % 2)
                .replace('<polygon', `<polygon fill="${col}"`);
              break;
            case 'flow':
              shapes += lens(X1 - (horiz ? 0.3 : 0), Y1 - (horiz ? 0 : 0.3), X2 + (horiz ? 0.3 : 0), Y2 + (horiz ? 0 : 0.3), w).replace('<path', `<path fill="${col}"`);
              break;
            case 'clerical': {
              if (horiz) {
                const hw = w / 2, tail = r === longestH;
                const x1 = X1 - 0.1, x2 = X2 + 0.15;
                let d = `M${f2(x1)} ${f2(Y1 - hw)}`;
                if (tail) {
                  d += `L${f2(x2 - 0.7)} ${f2(Y1 - hw)}C${f2(x2 - 0.1)} ${f2(Y1 - hw)} ${f2(x2 + 0.25)} ${f2(Y1 - 0.95 * w)} ${f2(x2 + 0.6)} ${f2(Y1 - 1.25 * w)}C${f2(x2 + 0.3)} ${f2(Y1 - 0.3 * w)} ${f2(x2 + 0.15)} ${f2(Y1 + 0.3 * w)} ${f2(x2 - 0.2)} ${f2(Y1 + hw)}`;
                } else d += `L${f2(x2 - 0.2)} ${f2(Y1 - hw)}L${f2(x2 - 0.1)} ${f2(Y1 + hw)}`;
                d += `L${f2(x1)} ${f2(Y1 + hw)}A${f2(hw)} ${f2(hw)} 0 0 1 ${f2(x1)} ${f2(Y1 - hw)}Z`;
                shapes += `<path d="${d}" fill="${col}"/>`;
              } else shapes += rect(X1 - 0.36 * w, Y1 - 0.05, 0.72 * w, len + 0.1, ` fill="${col}"`);
              break;
            }
            case 'regular': {
              if (horiz) {
                shapes += `<g transform="rotate(-4 ${f2(X1)} ${f2(Y1)})">${rect(X1 - 0.15, Y1 - 0.26 * w, len - 0.1, 0.52 * w, ` fill="${col}"`)}<circle cx="${f2(X2 - 0.12)}" cy="${f2(Y1 + 0.06 * w)}" r="${f2(0.44 * w)}" fill="${col}"/></g>`;
              } else {
                shapes += rect(X1 - 0.42 * w, Y1 - 0.15, 0.84 * w, len + 0.15, ` fill="${col}"`);
                if (r === longestV) shapes += `<polygon points="${f2(X1 + 0.42 * w)},${f2(Y2 - 0.55)} ${f2(X1 + 0.42 * w)},${f2(Y2 + 0.08)} ${f2(X1 - 0.85)},${f2(Y2 - 0.42)}" fill="${col}"/>`;
              }
              break;
            }
            case 'broken': {
              const units = Math.round(len / (horiz ? p * ax : p * ay));
              const cut = units >= 2 && xb[xk % 64] ? 1 + (xb[(xk + 1) % 64] ? 1 : 0) % units : 0;
              xk += 2;
              const parts = cut ? [[0, cut], [cut, units]] : [[0, units]];
              for (const [a, c] of parts) {
                if (c <= a) continue;
                const pp = horiz ? p * ax : p * ay;
                const sX1 = horiz ? X1 + a * pp + (a ? 0.22 : -w / 2) : X1;
                const sX2 = horiz ? X1 + c * pp - (c < units ? 0.22 : -w / 2) : X2;
                const sY1 = horiz ? Y1 : Y1 + a * pp + (a ? 0.22 : -w / 2);
                const sY2 = horiz ? Y2 : Y1 + c * pp - (c < units ? 0.22 : -w / 2);
                shapes += zigzag(sX1, sY1, sX2, sY2, w * (0.85 + 0.3 * xb[(xk + 3) % 64]), 0.6, 0.14, (a + r.a) % 2).replace('<polygon', `<polygon fill="${col}"`);
              }
              break;
            }
          }
        }
      }
    /* frame */
    let frame = '';
    let clip = '';
    const cid = `clip-${o.id}-${hex.slice(2, 8)}`;
    if (o.shape === 'round') {
      const R = Math.min(W, H) / 2;
      const cx = W / 2, cy = H / 2;
      const inner = R - wf - 0.6 * u;
      clip = `<clipPath id="${cid}"><circle cx="${f2(cx)}" cy="${f2(cy)}" r="${f2(inner)}"/></clipPath>`;
      shapes = `<g clip-path="url(#${cid})">${shapes}</g>`;
      if (o.style === 'bai') frame = '';
      else frame = `<circle cx="${f2(cx)}" cy="${f2(cy)}" r="${f2(R - wf / 2)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    } else if (o.frame === 'none') frame = '';
    else if (o.frame === 'double' && o.style !== 'bai') {
      const t = 0.45 * w0, inset = wf + 0.9 * u;
      frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.5 * wf)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/><rect x="${f2(inset + t / 2)}" y="${f2(inset + t / 2)}" width="${f2(W - 2 * inset - t)}" height="${f2(H - 2 * inset - t)}" fill="none" stroke="${o.ink}" stroke-width="${f2(t)}"/>`;
    } else if (o.style !== 'bai') {
      if (o.hand === 'clay') frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(1.6 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
      else if (o.hand === 'broken') {
        const j = (k) => (xb[k % 64] ? 0.35 : -0.25);
        const i = wf / 2;
        const c = [[i + j(1), i + j(2)], [W - i + j(3), i + j(4)], [W - i + j(5), H - i + j(6)], [i + j(7), H - i + j(8)]];
        for (let k = 0; k < 4; k++) {
          const A = c[k], B = c[(k + 1) % 4];
          frame += `<line x1="${f2(A[0])}" y1="${f2(A[1])}" x2="${f2(B[0])}" y2="${f2(B[1])}" stroke="${o.ink}" stroke-width="${f2(wf * (0.8 + 0.6 * xb[(k + 9) % 64]))}" stroke-linecap="square"/>`;
        }
      } else if (o.hand === 'wire') frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.8 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
      else frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.5 * wf)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    }
    const field = o.style !== 'bai' ? ''
      : o.shape === 'round' ? `<circle cx="${f2(W / 2)}" cy="${f2(H / 2)}" r="${f2(Math.min(W, H) / 2)}" fill="${o.ink}"/>`
      : `<rect x="0" y="0" width="${f2(W)}" height="${f2(H)}" rx="${f2(0.4 * u)}" fill="${o.ink}"/>`;
    /* paste roughness; the clay hand wobbles softly instead */
    const fid = `hand-${o.id}-${o.hand}-${hex.slice(2, 8)}`;
    const useRough = o.rough && size >= 40;
    let filter = '';
    if (useRough) {
      const soft = o.hand === 'clay';
      const periodPx = soft ? 7 : 3, dispPx = soft ? 1.4 : o.hand === 'broken' ? 2.4 : o.roughPx;
      filter = `<filter id="${fid}" x="-6%" y="-6%" width="112%" height="112%"><feTurbulence type="fractalNoise" baseFrequency="${(pxPerUnit / periodPx).toFixed(3)}" numOctaves="2" seed="7" result="n"/><feDisplacementMap in="SourceGraphic" in2="n" scale="${(2 * dispPx / pxPerUnit).toFixed(2)}" xChannelSelector="R" yChannelSelector="G"/></filter>`;
    }
    const tilt = o.tilt || (o.hand === 'broken' ? -3 : 0);
    const pxW = Math.round(W * scale), pxH = Math.round(H * scale);
    return `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${f2(W)} ${f2(H)}" role="img" aria-label="Hash seal ${hex.slice(0, 10)}"${tilt ? ` style="transform: rotate(${tilt}deg)"` : ''}>${clip}<g${useRough ? ` filter="url(#${fid})"` : ''}>${field}${frame}${shapes}</g>${filter}</svg>`;
  }

  return { HANDS, render, runs, paths };
});
