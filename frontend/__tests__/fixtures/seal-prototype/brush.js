/* The brush: a polyline becomes a filled shape with the width of a loaded brush — an entry
   (hidden tip, 藏锋, or exposed tip, 露锋), a press at the turns (顿), a lift at the end (出锋,
   the tip drawn out) or a stop (回锋), thinner horizontals, a thinning in the middle of a fast
   stroke, threads between strokes where the brush barely left the paper (牵丝), dry streaks in
   a fast stroke (飞白). Then the seal renderer on top of seal2's generators: the binary tree
   chained into brush strokes (root to leaf: down and to the right, as the brush goes), the
   running stitch as pressed-and-lifted lines or as single dashes. Browser (window.Brush) and
   node. */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory(require('./seal2.js'), require('./seal3.js'));
  else root.Brush = factory(root.Seal2, root.Seal3);
})(typeof self !== 'undefined' ? self : this, function (S2, S3) {
  const f2 = (v) => (Math.round(v * 100) / 100).toString();
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const smooth = (t) => { t = clamp(t, 0, 1); return t * t * (3 - 2 * t); };
  function rng(seed) {
    let s = (seed >>> 0) || 1;
    return () => { s ^= s << 13; s >>>= 0; s ^= s >> 17; s ^= s << 5; s >>>= 0; return (s % 100000) / 100000; };
  }

  /* polyline → pieces (lines, quadratics at the corners; or one spline through every point) →
     dense samples with arc length and tangent */
  function sample(P, o) {
    const pieces = [];
    let cur = P[0];
    if (o.curve === 'spline' && P.length > 2) {
      const k = o.tension == null ? 6 : o.tension;
      const pts = [], cornerS = [];
      let s = 0;
      const push = (x, y) => { if (pts.length) { const l = pts[pts.length - 1]; s += Math.hypot(x - l.x, y - l.y); } pts.push({ x, y, s }); };
      push(P[0][0], P[0][1]);
      for (let i = 0; i < P.length - 1; i++) {
        const p0 = P[Math.max(0, i - 1)], p1 = P[i], p2 = P[i + 1], p3 = P[Math.min(P.length - 1, i + 2)];
        const c1 = [p1[0] + (p2[0] - p0[0]) / k, p1[1] + (p2[1] - p0[1]) / k];
        const c2 = [p2[0] - (p3[0] - p1[0]) / k, p2[1] - (p3[1] - p1[1]) / k];
        const m = o.spm || 8;
        for (let j = 1; j <= m; j++) {
          const t = j / m, u = 1 - t;
          push(u * u * u * p1[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t * t * t * p2[0], u * u * u * p1[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t * t * t * p2[1]);
        }
        if (i + 2 < P.length) {
          const a = [p1[0] - p0[0], p1[1] - p0[1]], b = [p3[0] - p2[0], p3[1] - p2[1]];
          const din = [p2[0] - p1[0], p2[1] - p1[1]];
          if (Math.abs(din[0] * b[1] - din[1] * b[0]) > 1e-6) cornerS.push(s);
          void a;
        }
      }
      for (let i = 0; i < pts.length; i++) {
        const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
        const tx = b.x - a.x, ty = b.y - a.y, l = Math.hypot(tx, ty) || 1;
        pts[i].tx = tx / l; pts[i].ty = ty / l;
      }
      return { pts, cornerS, L: s };
    }
    for (let i = 1; i < P.length - 1; i++) {
      const A = P[i - 1], C = P[i], B = P[i + 1];
      const vin = [C[0] - A[0], C[1] - A[1]], vout = [B[0] - C[0], B[1] - C[1]];
      const lin = Math.hypot(vin[0], vin[1]), lout = Math.hypot(vout[0], vout[1]);
      if (lin < 1e-9 || lout < 1e-9) continue;
      const din = [vin[0] / lin, vin[1] / lin], dout = [vout[0] / lout, vout[1] / lout];
      const cross = din[0] * dout[1] - din[1] * dout[0], dot = din[0] * dout[0] + din[1] * dout[1];
      if (Math.abs(cross) < 1e-6 && dot > 0) continue;
      const r = Math.min(o.r, lin / 2, lout / 2);
      const p1 = [C[0] - din[0] * r, C[1] - din[1] * r], p2 = [C[0] + dout[0] * r, C[1] + dout[1] * r];
      const mid = [(p1[0] + p2[0]) / 2, (p1[1] + p2[1]) / 2];
      const ctl = [C[0] + (C[0] - mid[0]) * o.over, C[1] + (C[1] - mid[1]) * o.over];
      pieces.push({ t: 'L', a: cur, b: p1 });
      pieces.push({ t: 'Q', a: p1, c: ctl, b: p2 });
      cur = p2;
    }
    pieces.push({ t: 'L', a: cur, b: P[P.length - 1] });
    const pts = [], cornerS = [];
    let s = 0;
    const push = (x, y) => { if (pts.length) { const l = pts[pts.length - 1]; s += Math.hypot(x - l.x, y - l.y); } pts.push({ x, y, s }); };
    for (const pc of pieces) {
      if (pc.t === 'L') {
        const len = Math.hypot(pc.b[0] - pc.a[0], pc.b[1] - pc.a[1]);
        if (!pts.length) push(pc.a[0], pc.a[1]);
        if (len < 1e-9) continue;
        const m = Math.max(1, Math.ceil(len / o.step));
        for (let i = 1; i <= m; i++) { const t = i / m; push(pc.a[0] + (pc.b[0] - pc.a[0]) * t, pc.a[1] + (pc.b[1] - pc.a[1]) * t); }
      } else {
        const m = o.qm || 10;
        for (let i = 1; i <= m; i++) {
          const t = i / m, u = 1 - t;
          push(u * u * pc.a[0] + 2 * u * t * pc.c[0] + t * t * pc.b[0], u * u * pc.a[1] + 2 * u * t * pc.c[1] + t * t * pc.b[1]);
          if (i === Math.ceil(m / 2)) cornerS.push(s);
        }
      }
    }
    for (let i = 0; i < pts.length; i++) {
      const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
      const tx = b.x - a.x, ty = b.y - a.y, l = Math.hypot(tx, ty) || 1;
      pts[i].tx = tx / l; pts[i].ty = ty / l;
    }
    return { pts, cornerS, L: s };
  }

  /* one brush stroke. P: polyline in user units. o: w (base width), r (corner radius), over
     (0 cuts the corner, 1 swings round it), entry 'round'|'point', exit 'taper'|'blunt'|'point',
     bulge (press at a turn), hmul (horizontal width / vertical), tail (length of the lift),
     thin (mid-stroke thinning on long strokes), mod (small life), unit, step, seed,
     press (per-unit pressure flags) + lift (width when lifted), wmin */
  function stroke(P, o) {
    const u = o.unit;
    const { pts, cornerS, L } = sample(P, Object.assign({ step: u / 6 }, o));
    const rnd = rng(o.seed || 1);
    const phase = rnd() * 6.283;
    /* the trembling line (涩笔): the brush held back, the centerline waving a little */
    if (o.tremor && L > 1.4 * u) {
      const ph2 = rnd() * 6.283;
      for (const p of pts) {
        const off = o.tremor.amp * u * Math.sin((p.s / u) * o.tremor.freq * 6.283 + ph2) * Math.min(1, p.s / (0.5 * u), (L - p.s) / (0.5 * u));
        p.x += -p.ty * off; p.y += p.tx * off;
      }
      for (let i = 0; i < pts.length; i++) {
        const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
        const tx = b.x - a.x, ty = b.y - a.y, l = Math.hypot(tx, ty) || 1;
        pts[i].tx = tx / l; pts[i].ty = ty / l;
      }
    }
    /* the exit may depend on the last direction: a horizontal stops with a knot, a vertical is drawn out */
    const last = pts[pts.length - 1];
    const horizEnd = Math.abs(last.tx) > Math.abs(last.ty);
    const exit = horizEnd && o.exitH ? o.exitH : !horizEnd && o.exitV ? o.exitV : o.exit;
    const sig = (o.bulgeSigma || 0.45) * u;
    const widths = pts.map((p) => {
      let w = o.w * (o.hmul + (1 - o.hmul) * Math.abs(p.ty));
      if (o.press) {
        const k = p.s / u, i = Math.floor(k), fr = k - i;
        const at = (j) => o.press[clamp(j, 0, o.press.length - 1)];
        const a = at(i), b = i + 1 < o.press.length ? at(i + 1) : a, c = i > 0 ? at(i - 1) : a;
        let pr = c + (a - c) * smooth((fr + 0.22) / 0.44);
        pr += (b - pr) * smooth((fr - 0.78) / 0.44);
        w *= o.lift + (1 - o.lift) * pr;
      }
      const ent = o.entryLen == null ? 0.55 * u : o.entryLen;
      if (o.entry === 'point') {
        w *= Math.pow(clamp(p.s / ent, 0, 1), 0.55);
        if (o.press0) w *= 1 + o.press0 * Math.exp(-Math.pow((p.s - ent * 1.1) / (0.3 * u), 2));
      } else w *= 1 + (o.press0 == null ? 0.1 : o.press0) * Math.exp(-Math.pow((p.s - 0.3 * u) / (0.32 * u), 2));
      const rem = L - p.s;
      if (exit === 'taper') w *= Math.pow(clamp(rem / o.tail, 0, 1), o.taperPow || 0.75);
      else if (exit === 'point') w *= Math.pow(clamp(rem / (0.5 * u), 0, 1), 0.7);
      else if (exit === 'knot') w *= 1 + ((o.knot || 2.2) - 1) * Math.exp(-Math.pow((rem - 0.1 * u) / (0.13 * u), 2));
      else w *= 1 + 0.2 * Math.exp(-Math.pow((rem - 0.18 * u) / (0.26 * u), 2));
      for (const sc of cornerS) w *= 1 + o.bulge * Math.exp(-Math.pow((p.s - sc) / sig, 2));
      if (o.thin && L > 2 * u) w *= 1 - o.thin * Math.pow(Math.sin(Math.PI * p.s / L), 2) * Math.min(1, (L - 2 * u) / (2 * u));
      if (o.pulse) w *= 1 - o.pulse * Math.pow(Math.sin((Math.PI * p.s) / u), 2);
      if (o.mod) w *= 1 + o.mod * Math.sin((p.s / u) * 2.1 + phase);
      return Math.max(w, o.wmin == null ? 0.03 * o.w : o.wmin);
    });
    const left = [], right = [];
    pts.forEach((p, i) => {
      const nx = -p.ty, ny = p.tx, h = widths[i] / 2;
      left.push([p.x + nx * h, p.y + ny * h]);
      right.push([p.x - nx * h, p.y - ny * h]);
    });
    let d = `M${f2(left[0][0])} ${f2(left[0][1])}`;
    for (let i = 1; i < left.length; i++) d += `L${f2(left[i][0])} ${f2(left[i][1])}`;
    for (let i = right.length - 1; i >= 0; i--) d += `L${f2(right[i][0])} ${f2(right[i][1])}`;
    d += 'Z';
    const caps = [];
    if (o.entry !== 'point') caps.push([pts[0].x, pts[0].y, widths[0] / 2]);
    if (exit === 'blunt' || exit === 'knot') caps.push([pts[pts.length - 1].x, pts[pts.length - 1].y, widths[widths.length - 1] / 2]);
    return { d, caps, pts, widths, L, start: pts[0], end: pts[pts.length - 1] };
  }

  const shape = (r, col, extra) => `<path d="${r.d}" fill="${col}"${extra || ''}/>` + r.caps.map((c) => `<circle cx="${f2(c[0])}" cy="${f2(c[1])}" r="${f2(c[2])}" fill="${col}"${extra || ''}/>`).join('');

  /* a thread from the end of one stroke to the start of the next: the brush lifts along its
     own direction and lands along the next stroke's; only forward (down or right), never back */
  function thread(E, S, w, maxDist, col, any) {
    const dx = S.x - E.x, dy = S.y - E.y, dist = Math.hypot(dx, dy);
    if (dist > maxDist || dist < 0.02) return '';
    if (!any && dx + dy < -0.2 * dist) return '';
    const k = 0.38 * dist;
    const c1 = [E.x + E.tx * k, E.y + E.ty * k], c2 = [S.x - S.tx * k, S.y - S.ty * k];
    return `<path d="M${f2(E.x)} ${f2(E.y)}C${f2(c1[0])} ${f2(c1[1])} ${f2(c2[0])} ${f2(c2[1])} ${f2(S.x)} ${f2(S.y)}" fill="none" stroke="${col}" stroke-width="${f2(w * 0.16)}" stroke-linecap="round" opacity=".85"/>`;
  }

  /* dry streaks (飞白): hairlines of the ground along a fast stroke */
  function dry(r, u, col) {
    if (r.L < 1.6 * u) return '';
    let out = '';
    for (const off of [-0.26, 0, 0.22]) {
      let d = '', on = false;
      r.pts.forEach((p, i) => {
        const t = p.s / r.L;
        if (t < 0.28 || t > 0.9 || r.widths[i] < 0.35 * u * 0.5) { on = false; return; }
        const nx = -p.ty, ny = p.tx, x = p.x + nx * off * r.widths[i], y = p.y + ny * off * r.widths[i];
        d += (on ? 'L' : 'M') + f2(x) + ' ' + f2(y);
        on = true;
      });
      if (d) out += `<path d="${d}" fill="none" stroke="${col}" stroke-width="${f2(0.07 * u)}" stroke-linecap="round"/>`;
    }
    return out;
  }

  /* --- the binary tree as strokes: the forest rooted at the top row and left column; each
     stroke runs from a root (or a junction on a stroke already written) down / right to a leaf --- */
  function treeStrokes(b, n, order) {
    const key = (x, y) => x + ',' + y;
    const parent = new Map(), children = new Map();
    let k = 0;
    for (let r = 1; r < n; r++)
      for (let c = 1; c < n; c++) {
        const v = b[k++], p = v ? [c, r - 1] : [c - 1, r];
        parent.set(key(c, r), p);
        const pk = key(p[0], p[1]);
        if (!children.has(pk)) children.set(pk, []);
        children.get(pk).push([c, r]);
      }
    const depth = new Map();
    const d = (x, y) => {
      const kk = key(x, y);
      if (depth.has(kk)) return depth.get(kk);
      const p = parent.get(kk);
      const v = p ? d(p[0], p[1]) + 1 : 0;
      depth.set(kk, v);
      return v;
    };
    const leaves = [];
    for (const kk of parent.keys()) if (!children.has(kk)) leaves.push(kk.split(',').map(Number));
    leaves.sort((A, B) => (order === 'long' ? d(B[0], B[1]) - d(A[0], A[1]) || A[1] - B[1] || A[0] - B[0] : A[1] - B[1] || A[0] - B[0]));
    const on = new Set();
    const out = [];
    for (const leaf of leaves) {
      const pts = [leaf];
      let cur = leaf;
      for (;;) {
        const p = parent.get(key(cur[0], cur[1]));
        if (!p) break;
        pts.push(p);
        if (on.has(key(p[0], p[1]))) break;
        cur = p;
      }
      for (const q of pts) on.add(key(q[0], q[1]));
      pts.reverse();
      out.push({ pts });
    }
    return out;
  }

  /* the running stitch: 'line' = every row and column one stroke, pressed on the dashes and
     lifted between them; 'dash' = every dash its own short stroke; 'wave' = every row and
     column one wave through the lattice, the bit setting which way it swings first, pressed on
     the crests (the dashes) and lifted in the troughs */
  function stitchStrokes(b, n, mode, amp) {
    const out = [];
    const A = amp == null ? 0.3 : amp;
    for (let r = 0; r < n; r++) {
      const ph = b[r], flags = [];
      for (let c = 0; c < n - 1; c++) flags.push((c + ph) % 2 === 0 ? 1 : 0);
      if (mode === 'line') out.push({ pts: [[0, r], [n - 1, r]], press: flags });
      else if (mode === 'wave') {
        const pts = [];
        for (let c = 0; c < n; c++) pts.push([c, r + ((c + ph) % 2 ? A : -A)]);
        out.push({ pts, press: flags, spline: true });
      } else flags.forEach((f, c) => { if (f) out.push({ pts: [[c, r], [c + 1, r]], horiz: true }); });
    }
    for (let c = 0; c < n; c++) {
      const ph = b[n + c], flags = [];
      for (let r = 0; r < n - 1; r++) flags.push((r + ph) % 2 === 0 ? 1 : 0);
      if (mode === 'line') out.push({ pts: [[c, 0], [c, n - 1]], press: flags });
      else if (mode === 'wave') {
        const pts = [];
        for (let r = 0; r < n; r++) pts.push([c + ((r + ph) % 2 ? A : -A), r]);
        out.push({ pts, press: flags, spline: true });
      } else flags.forEach((f, r) => { if (f) out.push({ pts: [[c, r], [c, r + 1]], horiz: false }); });
    }
    return out;
  }

  /* writing order: greedy, from the top left, preferring the next stroke down and to the right */
  function orderStrokes(strokes) {
    if (strokes.length < 2) return strokes;
    const rem = strokes.slice();
    rem.sort((a, b) => a.pts[0][1] + 0.5 * a.pts[0][0] - (b.pts[0][1] + 0.5 * b.pts[0][0]));
    let cur = rem.shift();
    const out = [cur];
    while (rem.length) {
      const e = cur.pts[cur.pts.length - 1];
      let bi = 0, bc = Infinity;
      rem.forEach((s, i) => {
        const st = s.pts[0], dx = st[0] - e[0], dy = st[1] - e[1];
        const c = Math.hypot(dx, dy) + 0.7 * Math.max(0, -dy) + 0.35 * Math.max(0, -dx);
        if (c < bc) { bc = c; bi = i; }
      });
      cur = rem.splice(bi, 1)[0];
      out.push(cur);
    }
    return out;
  }

  const HANDS = {
    running: { label: 'running (行)', w: 0.9, r: 0.44, over: 0.4, entry: 'round', exit: 'taper', bulge: 0.16, hmul: 0.76, tail: 0.95, thin: 0.06, mod: 0, threads: false, order: 'order', slant: 0, dry: false, curve: 'corner', dotW: 1.15 },
    cursive: { label: 'cursive (草)', w: 0.96, r: 0.5, over: 0.6, entry: 'point', exit: 'taper', bulge: 0.22, hmul: 0.8, tail: 0.95, thin: 0.14, mod: 0, threads: true, order: 'long', slant: -3, dry: false, curve: 'spline', tension: 5, dotW: 1.15 },
    wild: { label: 'wild cursive (狂草)', w: 0.9, r: 0.5, over: 0.75, entry: 'point', exit: 'taper', bulge: 0.3, hmul: 0.7, tail: 1.4, thin: 0.32, mod: 0.06, threads: true, order: 'long', slant: -6, dry: true, curve: 'spline', tension: 4, dotW: 1.15 },
  };

  function render(hex, size, o) {
    o = Object.assign({
      gen: 'bt', n: 9, ratio: 0.7, hand: 'running', style: 'bai', frame: 'single', framew: 2.5, pad: 1.6, aspect: [1, 1],
      ink: '#c2452d', ground: '#15120f', rough: true, roughPx: 1.1, id: 'b', tilt: 0, maxThread: 2.4, stitch: 'line', seed: 3, brush: null, lift: 0.12,
    }, o || {});
    const hand = Object.assign({}, HANDS[o.hand], o.brush || {});
    const gen = S2.GEN[o.gen];
    const b = S2.bits(hex, gen.bits(o.n));
    const xb = S2.bits(hex, 64, 64);
    const u = 1, w0 = o.ratio, p = u + w0;
    const [ax, ay] = o.aspect;
    const spanX = (o.n - 1) * p * ax + w0, spanY = (o.n - 1) * p * ay + w0;
    const wf = o.style === 'bai' || o.style === 'ink' || o.frame === 'none' ? 0 : w0 * o.framew;
    const pad = o.pad * u;
    const W = 2 * wf + 2 * pad + spanX, H = 2 * wf + 2 * pad + spanY;
    const ox = wf + pad + w0 / 2, oy = wf + pad + w0 / 2;
    const X = (x) => ox + x * p * ax, Y = (y) => oy + y * p * ay;
    const col = o.style === 'bai' ? o.ground : o.ink;
    let strokes = o.gen === 'bt' ? treeStrokes(b, o.n, hand.order) : stitchStrokes(b, o.n, o.stitch, o.amp);
    strokes = orderStrokes(strokes);
    let shapes = '', threads = '', streaks = '';
    let prev = null;
    strokes.forEach((st, k) => {
      const P = st.pts.map(([x, y]) => [X(x), Y(y)]);
      let over = st.horiz === true ? { entry: 'point', entryLen: 0.4 * p, exit: 'blunt', curve: 'corner' } : st.horiz === false ? { entry: 'round', press0: 0.2, exit: 'taper', tail: 0.7 * p, curve: 'corner' } : {};
      if (st.horiz === true) { P[0][1] += 0.1 * p; P[1][1] -= 0.08 * p; }
      if (st.spline) over = { curve: 'spline', tension: o.tension || 4 };
      /* a single unit is a dot (点): the tip lands thin and the brush presses at the end, the
         dot leaning a little, as a written dot does; alternate dots lean the other way */
      if (st.pts.length === 2 && !st.press && Math.hypot(P[1][0] - P[0][0], P[1][1] - P[0][1]) <= 1.05 * p) {
        const vert = Math.abs(P[1][1] - P[0][1]) > Math.abs(P[1][0] - P[0][0]);
        const lean = (st.pts[0][0] + st.pts[0][1]) % 2 ? 0.26 : -0.18;
        if (vert) { P[0][0] -= lean * p * 0.4; P[1][0] += lean * p * 0.6; P[1][1] -= 0.08 * p; }
        else { P[0][1] -= lean * p * 0.4; P[1][1] += lean * p * 0.6; P[1][0] -= 0.08 * p; }
        over = Object.assign({}, over, { curve: 'corner', entry: 'point', entryLen: 0.5 * p, exit: 'blunt', w: w0 * hand.w * (hand.dotW || 1.1), thin: 0, bulge: 0 });
      }
      const r = stroke(P, Object.assign({}, hand, { w: w0 * hand.w, r: hand.r * p, unit: p, seed: o.seed * 7919 + k * 131 + 1, press: st.press || null, lift: o.lift }, over));
      shapes += shape(r, col);
      if (hand.threads && prev) threads += thread(prev.end, r.start, w0 * hand.w, o.maxThread * p, col, o.anyThread);
      if (hand.dry) streaks += dry(r, p, o.style === 'bai' ? o.ink : o.ground);
      prev = r;
    });
    /* frame and field */
    let frame = '';
    if (o.style === 'zhu') {
      if (o.frame === 'worn') {
        const eb = xb, segs = [], inset = wf / 2;
        const sides = [[inset, inset, W - inset, inset], [W - inset, inset, W - inset, H - inset], [W - inset, H - inset, inset, H - inset], [inset, H - inset, inset, inset]];
        let i = 0;
        for (const [x1, y1, x2, y2] of sides)
          for (let q = 0; q < 4; q++, i++) {
            if (eb[i] && eb[i + 1]) continue;
            const a = [x1 + ((x2 - x1) * q) / 4, y1 + ((y2 - y1) * q) / 4], c = [x1 + ((x2 - x1) * (q + 1)) / 4, y1 + ((y2 - y1) * (q + 1)) / 4];
            segs.push(`M${f2(a[0])} ${f2(a[1])}L${f2(c[0])} ${f2(c[1])}`);
          }
        frame = `<path d="${segs.join('')}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}" stroke-linecap="butt"/>`;
      } else if (o.frame !== 'none') frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.7 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    }
    const field = o.style === 'bai' ? `<rect x="0" y="0" width="${f2(W)}" height="${f2(H)}" rx="${f2(0.4 * u)}" fill="${o.ink}"/>` : '';
    const slant = hand.slant ? Math.tan((hand.slant * Math.PI) / 180) : 0;
    const g = slant ? `<g transform="matrix(1 ${f2(slant)} 0 1 0 ${f2(-slant * (W / 2))})">` : '<g>';
    const scale = size / Math.max(W, H);
    const fid = `brush-${o.id}-${o.hand}-${hex.slice(2, 8)}`;
    const useRough = o.rough && size >= 40 && o.style !== 'ink';
    const filter = useRough
      ? `<filter id="${fid}" x="-6%" y="-6%" width="112%" height="112%"><feTurbulence type="fractalNoise" baseFrequency="${(scale / 3).toFixed(3)}" numOctaves="2" seed="7" result="n"/><feDisplacementMap in="SourceGraphic" in2="n" scale="${(2 * o.roughPx / scale).toFixed(2)}" xChannelSelector="R" yChannelSelector="G"/></filter>`
      : '';
    const pxW = Math.round(W * scale), pxH = Math.round(H * scale);
    return `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${f2(W)} ${f2(H)}" role="img" aria-label="Hash seal ${hex.slice(0, 10)}"${o.tilt ? ` style="transform: rotate(${o.tilt}deg)"` : ''}><g${useRough ? ` filter="url(#${fid})"` : ''}>${field}${frame}${g}${streaks ? '' : ''}${shapes}${threads}${streaks}</g></g>${filter}</svg>`;
  }

  /* --- the Song hands: stroke habits taken from the masters' own sheets ---
     瘦金 (Huizong, 《秾芳诗》《千字文》): a thin hard line, the tip exposed at every entry, a
       knot (竹节) at every turn, the horizontal stopping on a knot, the vertical drawn out.
     苏 (Su Shi, 《寒食帖》): plump and flat, the brush lying on its side; horizontals heavier
       than verticals, turns soft and round, every stroke short and full; a squat character.
     黄 (Huang Tingjian, 《松风阁》): the centre tight and the long strokes thrown outward past
       the character like oars, trembling along their length (涩笔).
     米 (Mi Fu, 《蜀素帖》): "brushed" — the tip lands at a slant and presses at once, the
       strokes thin and thicken with speed, the verticals flick into a hook, every character
       leans its own way (欹侧), threads between the strokes.
     蔡 (Cai Xiang, 《澄心堂帖》): even, courteous, round; nothing exaggerated.
     黄草 (Huang's cursive, 《诸上座帖》): loops, long swings, thinning hard, dots. */
  const SONG = {
    shoujin: { label: '瘦金 Huizong', w: 0.44, curve: 'corner', r: 0.12, over: 0, entry: 'point', entryLen: 0.3, press0: 0, exitH: 'knot', exitV: 'taper', exit: 'taper', knot: 2.8, bulge: 1.5, bulgeSigma: 0.14, hmul: 0.95, tail: 0.55, thin: 0.06, mod: 0, threads: false, order: 'order', slant: -1, dotW: 1.7, ext: 0.22, tilt: 0, weight: 0.05, gens: ['bt'], ns: [4, 5, 6, 5] },
    su: { label: '苏 Su Shi', w: 1.15, curve: 'corner', r: 0.42, over: 0.3, entry: 'round', press0: 0.15, exit: 'blunt', bulge: 0.1, hmul: 1.14, tail: 0.8, thin: 0.05, mod: 0.04, threads: false, order: 'order', slant: 2, dotW: 1.15, ext: 0.08, tilt: 2, weight: 0.14, gens: ['bt', 'walk'], ns: [4, 5, 4, 5] },
    huang: { label: '黄 Huang Tingjian', w: 0.74, curve: 'corner', r: 0.3, over: 0.25, entry: 'point', entryLen: 0.5, press0: 0.2, exit: 'taper', tail: 1.1, bulge: 0.25, hmul: 0.8, thin: 0.12, mod: 0, tremor: { amp: 0.06, freq: 1.5 }, threads: false, order: 'long', slant: -4, dotW: 1.1, ext: 0.8, tilt: 3, weight: 0.1, gens: ['bt', 'btMirror'], ns: [4, 5, 5, 6] },
    mi: { label: '米 Mi Fu', w: 0.88, curve: 'spline', tension: 5, entry: 'point', entryLen: 0.32, press0: 0.35, exit: 'taper', tail: 0.9, bulge: 0.3, hmul: 0.78, thin: 0.22, mod: 0.06, threads: true, order: 'long', slant: -5, dotW: 1.15, hook: 'v', ext: 0.3, tilt: 6, weight: 0.16, gens: ['bt', 'walk', 'bt', 'stitch'], ns: [4, 5, 6, 5] },
    cai: { label: '蔡 Cai Xiang', w: 0.82, curve: 'corner', r: 0.36, over: 0.35, entry: 'round', press0: 0.08, exit: 'taper', tail: 0.9, bulge: 0.14, hmul: 0.8, thin: 0.06, mod: 0, threads: false, order: 'order', slant: 0, dotW: 1.1, ext: 0.08, tilt: 0, weight: 0.06, gens: ['bt'], ns: [5, 5, 4, 5] },
    huangcao: { label: '黄 cursive', w: 0.82, curve: 'spline', tension: 3.6, entry: 'point', entryLen: 0.5, press0: 0.1, exit: 'taper', tail: 1.2, bulge: 0.28, hmul: 0.85, thin: 0.3, mod: 0.05, tremor: { amp: 0.035, freq: 1.2 }, threads: true, order: 'long', slant: -3, dotW: 1.2, ext: 0.5, tilt: 4, weight: 0.12, gens: ['bt', 'walk'], ns: [4, 5, 5, 6] },
  };

  /* unit segments of any generator → polylines, oriented down / right as a brush goes */
  function chainSegs(segs) {
    return S3.paths(segs).map((pts) => {
      const a = pts[0], b = pts[pts.length - 1];
      return { pts: b[0] + b[1] < a[0] + a[1] ? pts.slice().reverse() : pts };
    });
  }

  /* The round-2 seal, 2×2 glyphs, in a Song hand, with the four glyphs made unalike: each takes
     its own lattice (4/5/6), its own generator, its own lean and weight from the hash, and the
     cells split unequally — a four-word seal, not a tile. */
  function seal(hex, size, o) {
    o = Object.assign({
      hand: 'shoujin', hands: null, style: 'zhu', frame: 'single', framew: 2.2, pad: 1.5, ratio: 0.7,
      ink: '#c2452d', ground: '#15120f', rough: true, roughPx: 1.1, id: 'sg', tiltAll: 0, vary: true, maxThread: 2.2,
      small: false,
    }, o || {});
    const all = S2.bits(hex, 256);
    let cur = 0;
    const take = (k) => { let v = 0; for (let i = 0; i < k; i++) v = (v << 1) | all[cur++]; return v; };
    const u = 1, w0 = o.ratio, p0 = u + w0;
    const base = 4 * p0; // the span of a 5×5 glyph
    const gap = p0;
    const wf = o.style === 'bai' || o.frame === 'none' ? 0 : w0 * o.framew;
    const pad = o.pad * u;
    const inner = 2 * base + gap;
    const W = 2 * wf + 2 * pad + inner, H = W;
    /* the split of the field between the two columns and the two rows */
    const SPLIT = [0.5, 0.44, 0.56, 0.5];
    const fx = o.vary ? SPLIT[take(2)] : 0.5, fy = o.vary ? SPLIT[take(2)] : 0.5;
    const colW = [fx * (inner - gap), (1 - fx) * (inner - gap)], rowH = [fy * (inner - gap), (1 - fy) * (inner - gap)];
    /* per glyph: hand, lattice, generator, lean, weight */
    const glyphs = [];
    for (let i = 0; i < 4; i++) {
      const handName = o.hands ? o.hands[i % o.hands.length] : o.hand;
      const hand = SONG[handName];
      let n = o.small ? 3 : o.vary ? hand.ns[take(2)] : 5;
      let g = o.small ? 'bt' : o.vary && hand.gens.length > 1 ? hand.gens[take(2) % hand.gens.length] : hand.gens[0];
      if ((g === 'btMirror' || g === 'quadMirror') && n % 2 === 0) g = 'bt';
      const lean = o.vary && hand.tilt ? (take(1) ? 1 : -1) * hand.tilt * (0.5 + 0.5 * take(1)) : 0;
      const weight = o.vary ? 1 + (take(1) ? 1 : -1) * hand.weight : 1;
      glyphs.push({ hand, handName, n, g, lean, weight });
    }
    const varBits = cur;
    let shapes = '';
    for (let i = 0; i < 4; i++) {
      const G = glyphs[i], hand = G.hand;
      const gx = i % 2, gy = i >> 1;
      const gen = S2.GEN[G.g];
      const per = gen.bits(G.n);
      const b = all.slice(cur, cur + per);
      cur += per;
      const cx0 = wf + pad + (gx ? colW[0] + gap : 0), cy0 = wf + pad + (gy ? rowH[0] + gap : 0);
      const cw = colW[gx], ch = rowH[gy];
      const m = 0.35 * w0; // margin inside the cell for the stroke's own width
      const px = (cw - 2 * m) / (G.n - 1), py = (ch - 2 * m) / (G.n - 1);
      const X = (x) => cx0 + m + x * px, Y = (y) => cy0 + m + y * py;
      const p = (px + py) / 2;
      const segs = gen(b, G.n);
      let strokes = G.g === 'bt' ? treeStrokes(b, G.n, hand.order) : chainSegs(segs);
      strokes = orderStrokes(strokes);
      const w = w0 * hand.w * G.weight;
      const col = o.style === 'bai' ? o.ground : o.ink;
      let gs = '', threads = '';
      let prev = null;
      strokes.forEach((st, k) => {
        const P = st.pts.map(([x, y]) => [X(x), Y(y)]);
        const units = st.pts.length - 1;
        let over = {};
        const single = units === 1 && Math.hypot(P[1][0] - P[0][0], P[1][1] - P[0][1]) <= 1.05 * Math.max(px, py);
        if (single) {
          const vert = Math.abs(P[1][1] - P[0][1]) > Math.abs(P[1][0] - P[0][0]);
          const lean = (st.pts[0][0] + st.pts[0][1]) % 2 ? 0.26 : -0.18;
          if (vert) { P[0][0] -= lean * p * 0.4; P[1][0] += lean * p * 0.6; P[1][1] -= 0.08 * p; }
          else { P[0][1] -= lean * p * 0.4; P[1][1] += lean * p * 0.6; P[1][0] -= 0.08 * p; }
          over = { curve: 'corner', entry: 'point', entryLen: 0.5 * p, exit: 'blunt', exitH: null, exitV: null, w: w * (hand.dotW || 1.1), thin: 0, bulge: 0, tremor: null };
        } else {
          /* the ends thrown past the lattice point (黄's oars), and the hook on a vertical (米) */
          if (hand.ext) {
            const d0 = [P[1][0] - P[0][0], P[1][1] - P[0][1]], l0 = Math.hypot(d0[0], d0[1]) || 1;
            const d1 = [P[P.length - 1][0] - P[P.length - 2][0], P[P.length - 1][1] - P[P.length - 2][1]], l1 = Math.hypot(d1[0], d1[1]) || 1;
            const e1 = hand.ext * Math.min(1, units / 4) * p, e0 = 0.35 * e1;
            P[0] = [P[0][0] - (d0[0] / l0) * e0, P[0][1] - (d0[1] / l0) * e0];
            P[P.length - 1] = [P[P.length - 1][0] + (d1[0] / l1) * e1, P[P.length - 1][1] + (d1[1] / l1) * e1];
          }
          if (hand.hook === 'v') {
            const a = P[P.length - 2], z = P[P.length - 1];
            if (Math.abs(z[1] - a[1]) > Math.abs(z[0] - a[0]) && z[1] > a[1]) P.push([z[0] - 0.32 * p, z[1] - 0.3 * p]);
          }
        }
        const r = stroke(P, Object.assign({}, hand, { w, r: hand.r * p, unit: p, seed: (varBits + i) * 7919 + k * 131 + 1, tremor: hand.tremor ? hand.tremor : null }, over));
        gs += shape(r, col);
        if (hand.threads && prev) threads += thread(prev.end, r.start, w, o.maxThread * p, col);
        prev = r;
      });
      const ccx = cx0 + cw / 2, ccy = cy0 + ch / 2;
      const slant = hand.slant ? Math.tan((hand.slant * Math.PI) / 180) : 0;
      const tf = `${G.lean ? `rotate(${f2(G.lean)} ${f2(ccx)} ${f2(ccy)}) ` : ''}${slant ? `matrix(1 ${f2(slant)} 0 1 0 ${f2(-slant * ccx)})` : ''}`;
      shapes += tf ? `<g transform="${tf}">${gs}${threads}</g>` : gs + threads;
    }
    const bitsUsed = cur;
    /* frame and field */
    let frame = '';
    if (o.style === 'zhu' && o.frame !== 'none') {
      if (o.frame === 'double') {
        const t = 0.4 * w0, inset = wf + 0.8 * u;
        frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.6 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/><rect x="${f2(inset + t / 2)}" y="${f2(inset + t / 2)}" width="${f2(W - 2 * inset - t)}" height="${f2(H - 2 * inset - t)}" fill="none" stroke="${o.ink}" stroke-width="${f2(t)}"/>`;
      } else frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.7 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    }
    const field = o.style === 'bai' ? `<rect x="0" y="0" width="${f2(W)}" height="${f2(H)}" rx="${f2(0.4 * u)}" fill="${o.ink}"/>` : '';
    const scale = size / Math.max(W, H);
    const fid = `song-${o.id}-${hex.slice(2, 8)}`;
    const useRough = o.rough && size >= 40;
    const filter = useRough
      ? `<filter id="${fid}" x="-6%" y="-6%" width="112%" height="112%"><feTurbulence type="fractalNoise" baseFrequency="${(scale / 3).toFixed(3)}" numOctaves="2" seed="7" result="n"/><feDisplacementMap in="SourceGraphic" in2="n" scale="${(2 * o.roughPx / scale).toFixed(2)}" xChannelSelector="R" yChannelSelector="G"/></filter>`
      : '';
    const pxW = Math.round(W * scale), pxH = Math.round(H * scale);
    const svg = `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${f2(W)} ${f2(H)}" role="img" aria-label="Hash seal ${hex.slice(0, 10)}"${o.tiltAll ? ` style="transform: rotate(${o.tiltAll}deg)"` : ''}><g${useRough ? ` filter="url(#${fid})"` : ''}>${field}${frame}${shapes}</g>${filter}</svg>`;
    return o.meta ? { svg, bitsUsed, glyphs: glyphs.map((g) => ({ hand: g.handName, n: g.n, gen: g.g, lean: Math.round(g.lean), weight: g.weight })), split: [fx, fy] } : svg;
  }

  /* --- ornaments: a pattern that looks like calligraphy, not a character ---
     every stroke is clean-edged (no paste roughness, no tremor), the brush even with a pulse of
     pressure at the lattice points and the tip drawn out at the ends. The hash sets the figure
     of the ornament; the ornament's own order (one line, a mirror, four-fold, rows) keeps it a
     pattern. */
  function walkRect(b, w, h, starts) {
    const s = [];
    const used = new Set();
    const key = (x, y) => x + ',' + y;
    const dirs = [[1, 0], [0, 1], [-1, 0], [0, -1]];
    let k = 0, n = 0;
    while (k < b.length && n < (starts || 2)) {
      let sx = -1, sy = -1;
      outer: for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) if (!used.has(key(x, y))) { sx = x; sy = y; break outer; }
      if (sx < 0) break;
      n++;
      let x = sx, y = sy, hd = 0;
      used.add(key(x, y));
      while (k < b.length) {
        const v = b[k++];
        const order = v ? [(hd + 3) % 4, (hd + 1) % 4, hd] : [hd, (hd + 3) % 4, (hd + 1) % 4];
        let moved = false;
        for (const d of order) {
          const nx = x + dirs[d][0], ny = y + dirs[d][1];
          if (nx < 0 || ny < 0 || nx >= w || ny >= h || used.has(key(nx, ny))) continue;
          s.push([x, y, nx, ny]);
          used.add(key(nx, ny));
          x = nx; y = ny; hd = d; moved = true;
          break;
        }
        if (!moved) break;
      }
    }
    return s;
  }

  const PATTERNS = {
    /* one line through the lattice, the brush never lifted: a flourish, like a cipher (花押) */
    flourish: { bits: (n) => 2 * (n - 1) * (n - 1), make: (b, n) => chainSegs(walkRect(b, n, n, 3)), curve: 'spline', tension: 3.2 },
    /* the same line on the left half, mirrored: a symmetric cipher */
    mirror: { bits: (n) => 2 * (n - 1) * ((n + 1) / 2), make: (b, n) => { const h = (n + 1) / 2; const segs = walkRect(b, h, n, 3); const all = segs.slice(); for (const [x1, y1, x2, y2] of segs) all.push([n - 1 - x1, y1, n - 1 - x2, y2]); return chainSegs(all); }, curve: 'spline', tension: 3.4 },
    /* four-fold symmetry, chained into curves: a window tracery, a rosette */
    rosette: { bits: (n) => ((n - 1) / 2) ** 2, make: (b, n) => chainSegs(S2.GEN.quadMirror(b, n)), curve: 'spline', tension: 4 },
    /* a curl in every cell, turned one of four ways by two bits: the cloud scroll */
    curls: { bits: (n) => 2 * (n - 1) * (n - 1), make: (b, n) => { const out = []; let k = 0; for (let r = 0; r < n - 1; r++) for (let c = 0; c < n - 1; c++) { const o = (b[k++] << 1) | b[k++]; const cx = c + 0.5, cy = r + 0.5; const pts = []; const dir = o & 1 ? 1 : -1; const a0 = (o >> 1 ? 0 : Math.PI) + (o & 1 ? 0 : Math.PI / 2); for (let i = 0; i <= 7; i++) { const a = a0 + dir * i * (Math.PI / 4); const rad = 0.5 - i * 0.052; pts.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)]); } out.push({ pts }); } return out; }, curve: 'spline', tension: 4, entry: 'round', press0: 0.1 },
    /* rows of waves, each row's phase and height from its bits: water */
    water: { bits: (n) => 2 * n, make: (b, n) => { const out = []; for (let r = 0; r < n; r++) { const ph = b[2 * r], amp = b[2 * r + 1] ? 0.36 : 0.22; const pts = []; for (let c = 0; c <= 2 * (n - 1); c++) { const x = c / 2; pts.push([x, r + amp * Math.sin(Math.PI * (x + (ph ? 0.5 : 0)))]); } out.push({ pts }); } return out; }, curve: 'spline', tension: 6, even: true },
    /* the rectangular spiral cut by the bits: the thunder meander, in a brush */
    meander: { bits: (n) => (n - 1) * (n - 1), make: (b, n) => chainSegs(S2.GEN.spiral(b, n)), curve: 'corner', r: 0.3, over: 0.2 },
    /* the running stitch as dots and dashes of the brush */
    dashes: { bits: (n) => 2 * n, make: (b, n) => stitchStrokes(b, n, 'dash'), curve: 'corner', r: 0.2, over: 0 },
  };

  function ornament(hex, size, o) {
    o = Object.assign({
      pattern: 'flourish', n: 7, style: 'zhu', frame: 'single', framew: 2.4, pad: 1.8, ratio: 0.6, w: 0.8, pulse: 0.24, bulge: 0.14,
      ink: '#c2452d', ground: '#15120f', id: 'o', tilt: 0, shape: 'square', cols: 1, rows: 1, tail: 1.2,
    }, o || {});
    const pat = PATTERNS[o.pattern];
    const per = pat.bits(o.n);
    const count = o.cols * o.rows;
    const all = S2.bits(hex, per * count);
    const u = 1, w0 = o.ratio, p = u + w0;
    const span = (o.n - 1) * p + w0;
    const gap = p;
    const wf = o.style === 'bai' || o.frame === 'none' ? 0 : w0 * o.framew;
    const pad = o.pad * u;
    const W = 2 * wf + 2 * pad + o.cols * span + (o.cols - 1) * gap;
    const H = 2 * wf + 2 * pad + o.rows * span + (o.rows - 1) * gap;
    const col = o.style === 'bai' ? o.ground : o.ink;
    let shapes = '';
    for (let gy = 0; gy < o.rows; gy++)
      for (let gx = 0; gx < o.cols; gx++) {
        const i = gy * o.cols + gx;
        const b = all.slice(i * per, (i + 1) * per);
        const ox = wf + pad + w0 / 2 + gx * (span + gap), oy = wf + pad + w0 / 2 + gy * (span + gap);
        const X = (x) => ox + x * p, Y = (y) => oy + y * p;
        let strokes = pat.make(b, o.n);
        if (!pat.even) strokes = orderStrokes(strokes);
        strokes.forEach((st, k) => {
          const P = st.pts.map(([x, y]) => [X(x), Y(y)]);
          const base = {
            w: w0 * o.w, r: (pat.r || 0.3) * p, over: pat.over == null ? 0.3 : pat.over, entry: pat.entry || 'point', entryLen: 0.6 * p, press0: pat.press0 || 0.08,
            exit: 'taper', tail: o.tail * p, bulge: pat.even ? 0 : o.bulge, hmul: pat.even ? 1 : 0.86, thin: 0, pulse: pat.even ? 0 : o.pulse, mod: 0,
            unit: p, seed: 1, curve: pat.curve, tension: pat.tension || 4, tremor: null,
          };
          if (st.horiz === true) Object.assign(base, { entry: 'point', entryLen: 0.4 * p, exit: 'blunt', pulse: 0 });
          if (st.horiz === false) Object.assign(base, { entry: 'round', press0: 0.2, exit: 'taper', tail: 0.7 * p, pulse: 0 });
          const r = stroke(P, base);
          shapes += shape(r, col);
        });
      }
    let frame = '', clip = '';
    const cid = `oclip-${o.id}-${hex.slice(2, 8)}`;
    if (o.shape === 'round') {
      const R = Math.min(W, H) / 2, cx = W / 2, cy = H / 2;
      clip = `<clipPath id="${cid}"><circle cx="${f2(cx)}" cy="${f2(cy)}" r="${f2(R - wf - 0.5 * u)}"/></clipPath>`;
      shapes = `<g clip-path="url(#${cid})">${shapes}</g>`;
      if (o.style !== 'bai') frame = `<circle cx="${f2(cx)}" cy="${f2(cy)}" r="${f2(R - wf / 2)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    } else if (o.style === 'zhu' && o.frame !== 'none') {
      frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.7 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
      if (o.frame === 'double') { const t = 0.4 * w0, inset = wf + 0.8 * u; frame += `<rect x="${f2(inset + t / 2)}" y="${f2(inset + t / 2)}" width="${f2(W - 2 * inset - t)}" height="${f2(H - 2 * inset - t)}" fill="none" stroke="${o.ink}" stroke-width="${f2(t)}"/>`; }
    }
    const field = o.style !== 'bai' ? '' : o.shape === 'round' ? `<circle cx="${f2(W / 2)}" cy="${f2(H / 2)}" r="${f2(Math.min(W, H) / 2)}" fill="${o.ink}"/>` : `<rect x="0" y="0" width="${f2(W)}" height="${f2(H)}" rx="${f2(0.4 * u)}" fill="${o.ink}"/>`;
    const scale = size / Math.max(W, H);
    const pxW = Math.round(W * scale), pxH = Math.round(H * scale);
    return `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${f2(W)} ${f2(H)}" role="img" aria-label="Hash seal ${hex.slice(0, 10)}"${o.tilt ? ` style="transform: rotate(${o.tilt}deg)"` : ''}>${clip}${field}${frame}${shapes}</svg>`;
  }

  /* --- compound ornaments and talismans ---
     a seal is a box of regions; each region takes a pattern (chosen by the hash where the
     layout allows) and draws it with the brush. A region is w×h units; its patterns are made
     for that box. The talisman (符箓) is the oldest "writing that is not words": a head of
     three strokes, stacked compartments each in its own figure, a knot and a sword at the foot. */
  const REGION = {
    curls: { bits: (w, h) => 2 * w * h, make: (b, w, h) => { const out = []; let k = 0; for (let r = 0; r < h; r++) for (let c = 0; c < w; c++) { const o = (b[k++] << 1) | b[k++]; const cx = c + 0.5, cy = r + 0.5, pts = []; const dir = o & 1 ? 1 : -1; const a0 = (o >> 1 ? 0 : Math.PI) + (o & 1 ? 0 : Math.PI / 2); for (let i = 0; i <= 7; i++) { const a = a0 + dir * i * (Math.PI / 4), rad = 0.5 - i * 0.052; pts.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)]); } out.push({ pts }); } return out; }, brush: { curve: 'spline', tension: 4, entry: 'round', press0: 0.1, pulse: 0.1 } },
    water: { bits: (w, h) => 2 * h, make: (b, w, h) => { const out = []; for (let r = 0; r < h; r++) { const ph = b[2 * r], amp = b[2 * r + 1] ? 0.3 : 0.18, pts = []; for (let c = 0; c <= 2 * w; c++) { const x = c / 2; pts.push([x, r + 0.5 + amp * Math.sin(Math.PI * (x + (ph ? 0.5 : 0)))]); } out.push({ pts }); } return out; }, brush: { curve: 'spline', tension: 6, even: true, hmul: 1 } },
    zigzag: { bits: (w, h) => 2 * h, make: (b, w, h) => { const out = []; for (let r = 0; r < h; r++) { const ph = b[2 * r], amp = b[2 * r + 1] ? 0.34 : 0.22, pts = []; for (let c = 0; c <= 2 * w; c++) pts.push([c / 2, r + 0.5 + ((c + ph) % 2 ? amp : -amp)]); out.push({ pts }); } return out; }, brush: { curve: 'corner', r: 0.06, over: 0, even: true, hmul: 1, bulge: 0.25, bulgeSigma: 0.2 } },
    dashes: { bits: (w, h) => w + h + 2, make: (b, w, h) => { const out = []; for (let r = 0; r <= h; r++) { const ph = b[r]; for (let c = 0; c < w; c++) if ((c + ph) % 2 === 0) out.push({ pts: [[c, r], [c + 1, r]], horiz: true }); } for (let c = 0; c <= w; c++) { const ph = b[h + 1 + c]; for (let r = 0; r < h; r++) if ((r + ph) % 2 === 0) out.push({ pts: [[c, r], [c, r + 1]], horiz: false }); } return out; }, brush: { curve: 'corner', r: 0.2, over: 0 } },
    bars: { bits: (w, h) => w * (h + 1), make: (b, w, h) => { const segs = []; let k = 0; for (let r = 0; r <= h; r++) for (let c = 0; c < w; c++) if (!b[k++]) segs.push([c, r, c + 1, r]); return chainSegs(segs).map((s) => Object.assign(s, { horiz: true })); }, brush: { curve: 'corner', r: 0.1, over: 0 } },
    cloud: { bits: (w, h) => 2 * (w + 1) * (h + 1), make: (b, w, h) => chainSegs(walkRect(b, w + 1, h + 1, 3)), brush: { curve: 'spline', tension: 3.2, bulge: 0.85, bulgeSigma: 0.2, pulse: 0.12 } },
    spiral: { bits: () => 2, make: (b, w, h) => { const cx = w / 2, cy = h / 2, R = Math.min(w, h) / 2 - 0.15, dir = b[0] ? 1 : -1, a0 = b[1] ? Math.PI : 0, pts = []; const turns = 2.4, N = 44; for (let i = 0; i <= N; i++) { const t = i / N, a = a0 + dir * t * turns * 2 * Math.PI, rad = R * (1 - 0.92 * t); pts.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)]); } return [{ pts }]; }, brush: { curve: 'spline', tension: 6, entry: 'round', press0: 0.15, pulse: 0, bulge: 0, tail: 2.2 } },
    rosette: { bits: (w, h) => { const m = Math.min(w, h) + 1; const n = m % 2 ? m : m - 1; return ((n - 1) / 2) ** 2; }, make: (b, w, h) => { const m = Math.min(w, h) + 1; const n = m % 2 ? m : m - 1; const ox = (w - (n - 1)) / 2, oy = (h - (n - 1)) / 2; return chainSegs(S2.GEN.quadMirror(b, n)).map((s) => ({ pts: s.pts.map(([x, y]) => [x + ox, y + oy]) })); }, brush: { curve: 'spline', tension: 4 } },
    meander: { bits: (w, h) => (w + 1) * (h + 1), make: (b, w, h) => { const pts = []; let left = 0, right = w, top = 0, bottom = h; pts.push([0, 0]); while (left <= right && top <= bottom) { if (pts.length > 1) pts.push([left, top]); for (let c = left + 1; c <= right; c++) pts.push([c, top]); for (let r = top + 1; r <= bottom; r++) pts.push([right, r]); if (top < bottom) for (let c = right - 1; c >= left; c--) pts.push([c, bottom]); if (left < right) for (let r = bottom - 1; r > top; r--) pts.push([left, r]); left++; right--; top++; bottom--; } const m = b.length, segs = []; for (let i = 0; i + 1 < pts.length; i++) { if (b[(i + 1) % m] && b[(i + 6) % m]) continue; const [x1, y1] = pts[i], [x2, y2] = pts[i + 1]; if (x1 === x2 && y1 === y2) continue; segs.push([x1, y1, x2, y2]); } return chainSegs(segs); }, brush: { curve: 'corner', r: 0.3, over: 0.2 } },
    /* the talisman's own figures: the ghost head (a spiral with two legs), the sun and moon
       (two rings with their points), the well (井), a row of hooks (勾), the chain of loops
       (罡), the lock (a box with its bar) */
    ghost: { bits: () => 3, make: (b, w, h) => { const cx = w / 2, R = Math.min(w / 2, h / 2) * 0.62, cy = R + 0.1, dir = b[0] ? 1 : -1, pts = []; for (let i = 0; i <= 26; i++) { const t = i / 26, a = Math.PI / 2 + dir * t * 1.9 * 2 * Math.PI, rad = R * (1 - 0.88 * t); pts.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)]); } const sp = b[1] ? 0.34 : 0.2, ly = Math.min(h, cy + R + 1.6); return [{ pts }, { pts: [[cx - R * 0.6, cy + R * 0.9], [cx - R * 0.6 - sp * w * 0.5, ly]], brush: { curve: 'corner', entry: 'round', press0: 0.2, exit: 'taper', tail: 0.9 } }, { pts: [[cx + R * 0.6, cy + R * 0.9], [cx + R * 0.6 + (b[2] ? sp : sp * 0.5) * w * 0.5, ly]], brush: { curve: 'corner', entry: 'round', press0: 0.2, exit: 'taper', tail: 0.9 } }]; }, brush: { curve: 'spline', tension: 6, entry: 'round', press0: 0.15, pulse: 0, bulge: 0, tail: 1.4 } },
    eyes: { bits: () => 3, make: (b, w, h) => { const r = Math.min(w / 4, h / 2) * (b[0] ? 0.78 : 0.62), cy = h / 2, out = []; for (const cx of [w / 4, (3 * w) / 4]) { const pts = []; for (let i = 0; i <= 12; i++) { const a = -Math.PI / 2 + (i / 12) * 2.05 * Math.PI; pts.push([cx + r * Math.cos(a), cy + r * Math.sin(a)]); } out.push({ pts }); const dx = (b[1] ? 0.22 : -0.22) * r, dy = (b[2] ? 0.2 : -0.1) * r; out.push({ pts: [[cx + dx - 0.12, cy + dy], [cx + dx + 0.12, cy + dy + 0.06]], brush: { curve: 'corner', entry: 'round', press0: 0.3, exit: 'blunt' } }); } return out; }, brush: { curve: 'spline', tension: 6, entry: 'point', entryLen: 0.5, exit: 'taper', tail: 0.9, pulse: 0, bulge: 0 } },
    well: { bits: () => 4, make: (b, w, h) => { const out = []; const xs = [w / 3, (2 * w) / 3], ys = [h / 3, (2 * h) / 3]; const e = 0.25; out.push({ pts: [[-e + (b[0] ? 0.3 : 0), ys[0]], [w + e, ys[0]]], horiz: true }); out.push({ pts: [[-e, ys[1]], [w + e - (b[1] ? 0.3 : 0), ys[1]]], horiz: true }); out.push({ pts: [[xs[0], -e + (b[2] ? 0.3 : 0)], [xs[0], h + e]], horiz: false }); out.push({ pts: [[xs[1], -e], [xs[1], h + e - (b[3] ? 0.3 : 0)]], horiz: false }); return out; }, brush: { curve: 'corner', r: 0.1 } },
    hooks: { bits: (w) => w, make: (b, w, h) => { const out = []; for (let i = 0; i < w; i++) { const x = i + 0.5, f = b[i] ? 1 : -1, top = 0.15, bot = h - 0.15; out.push({ pts: [[x, top], [x, bot - 0.45], [x + f * 0.42, bot], [x + f * 0.5, bot - 0.5]], brush: { curve: 'corner', r: 0.3, over: 0.3, entry: 'round', press0: 0.25, exit: 'taper', tail: 0.5 } }); } return out; }, brush: { curve: 'corner' } },
    loops: { bits: () => 2, make: (b, w, h) => { const n = Math.max(2, Math.round(w / 1.6)), cy = h / 2, r = Math.min(h / 2 - 0.1, 0.7), dir = b[0] ? 1 : -1, pts = []; const step = w / n; for (let i = 0; i < n; i++) { const cx = (i + 0.5) * step; for (let k = 0; k <= 8; k++) { const a = Math.PI + (k / 8) * 2 * Math.PI * dir * (i % 2 ? -1 : 1); pts.push([cx + r * 0.8 * Math.cos(a), cy + r * Math.sin(a)]); } } return [{ pts: b[1] ? pts : pts.slice().reverse() }]; }, brush: { curve: 'spline', tension: 5, entry: 'point', exit: 'taper', tail: 1.2, pulse: 0.1, bulge: 0 } },
    lock: { bits: () => 2, make: (b, w, h) => { const m = 0.25, out = [{ pts: [[m, m], [w - m, m], [w - m, h - m], [m, h - m], [m, m + 0.2]], brush: { curve: 'corner', r: 0.15, over: 0, entry: 'round', press0: 0.1, exit: 'blunt', pulse: 0 } }]; if (b[0]) out.push({ pts: [[-0.2, h / 2], [w + 0.2, h / 2]], horiz: true }); else out.push({ pts: [[w / 2, -0.2], [w / 2, h + 0.2]], horiz: false }); if (b[1]) out.push({ pts: [[w / 2 - 0.3, h / 2 - 0.3], [w / 2 + 0.3, h / 2 + 0.3]], brush: { curve: 'corner', entry: 'round', press0: 0.3, exit: 'blunt' } }); return out; }, brush: { curve: 'corner' } },
    /* the key border (回纹) along the inside of a ring one unit wide: hooks of two units, each flipped by a bit */
    key: { bits: (w, h) => w + h, make: (b, w, h) => { const out = []; let k = 0; const hook = (x, y, dx, dy, flip) => { const nx = -dy, ny = dx; const f = flip ? -1 : 1; return { pts: [[x, y], [x + dx * 2, y + dy * 2], [x + dx * 2 + nx * f, y + dy * 2 + ny * f], [x + dx + nx * f, y + dy + ny * f], [x + dx + nx * f * 0.45, y + dy + ny * f * 0.45]] }; }; for (let x = 0; x + 2 <= w; x += 2) out.push(hook(x, 0, 1, 0, !b[k++ % b.length])); for (let y = 0; y + 2 <= h; y += 2) out.push(hook(w, y, 0, 1, !b[k++ % b.length])); for (let x = w; x - 2 >= 0; x -= 2) out.push(hook(x, h, -1, 0, !b[k++ % b.length])); for (let y = h; y - 2 >= 0; y -= 2) out.push(hook(0, y, 0, -1, !b[k++ % b.length])); return out; }, brush: { curve: 'corner', r: 0.22, over: 0.1, pulse: 0 } },
  };
  const REGION_NAMES = Object.keys(REGION);

  /* layouts: W×H units, a list of regions {x, y, w, h, pattern | choose:[...]} and extra strokes
     (lattice units) drawn in the plain brush */
  const LAYOUTS = {
    /* the talisman: head ☰, four compartments of heights set by bits, dividers, the knot and the sword */
    talisman: (take) => {
      const W = 6, H = 17;
      const regions = [];
      const extra = [];
      const bar = (y, x0, x1) => extra.push({ pts: [[x0, y], [x1, y]], horiz: true });
      bar(0.4, 0.4, 5.6); bar(1.35, 1.2, 4.8); bar(2.3, 0.4, 5.6);
      let y = 3.2;
      const hs = [2, 3, 2, 3];
      for (let i = 0; i < 4; i++) {
        const h = hs[(i + take(1)) % 4];
        regions.push({ x: 0.5, y, w: W - 1, h, choose: ['cloud', 'zigzag', 'bars', 'curls', 'water', 'dashes', 'rosette', 'meander'] });
        y += h + 0.45;
        if (i < 3) { bar(y - 0.1, 0.9, 5.1); y += 0.35; }
      }
      const kx = W / 2, ky = y + 1.05;
      const pts = []; const dir = take(1) ? 1 : -1;
      for (let i = 0; i <= 30; i++) { const t = i / 30, a = dir * t * 2.2 * 2 * Math.PI, rad = 1.05 * (1 - 0.9 * t); pts.push([kx + rad * Math.cos(a), ky + rad * Math.sin(a)]); }
      extra.push({ pts, brush: { curve: 'spline', tension: 6, entry: 'round', press0: 0.1, tail: 1.6 } });
      extra.push({ pts: [[kx, ky + 1.3], [kx, H - 0.3], [kx - 0.5, H - 0.75]], brush: { curve: 'corner', r: 0.15, over: 0, entry: 'round', press0: 0.3, tail: 1.1, exit: 'taper' } });
      return { W, H, regions, extra };
    },
    /* the talisman folded into a square: head, two compartments side by side, the knot and sword below */
    talismanSquare: (take) => {
      const W = 9, H = 9;
      const regions = [], extra = [];
      const bar = (y, x0, x1) => extra.push({ pts: [[x0, y], [x1, y]], horiz: true });
      bar(0.4, 0.5, 8.5); bar(1.2, 1.6, 7.4);
      const split = take(1) ? 4 : 5;
      regions.push({ x: 0.5, y: 2, w: split - 0.75, h: 4, choose: ['cloud', 'zigzag', 'bars', 'curls', 'water', 'rosette', 'meander', 'dashes'] });
      regions.push({ x: split + 0.25, y: 2, w: 8.5 - split - 0.25, h: 4, choose: ['cloud', 'zigzag', 'bars', 'curls', 'water', 'rosette', 'meander', 'dashes'] });
      bar(6.5, 0.9, 8.1);
      const kx = 3, ky = 7.8, pts = []; const dir = take(1) ? 1 : -1;
      for (let i = 0; i <= 24; i++) { const t = i / 24, a = dir * t * 2 * 2 * Math.PI, rad = 0.85 * (1 - 0.9 * t); pts.push([kx + rad * Math.cos(a), ky + rad * Math.sin(a)]); }
      extra.push({ pts, brush: { curve: 'spline', tension: 6, entry: 'round', press0: 0.1, tail: 1.2 } });
      extra.push({ pts: [[4.6, 7.1], [8.3, 7.1], [8.3, 8.7], [7.7, 8.5]], brush: { curve: 'corner', r: 0.2, over: 0, entry: 'round', press0: 0.2, tail: 0.8 } });
      return { W, H, regions, extra };
    },
    /* a border of one pattern around a centre of another */
    border: (take) => {
      const W = 10, H = 10;
      const borders = ['key', 'dashes', 'water', 'zigzag'];
      const centres = ['curls', 'rosette', 'cloud', 'spiral'];
      return { W, H, regions: [{ x: 0, y: 0, w: W, h: H, pattern: borders[take(2)], ring: true }, { x: 2, y: 2, w: W - 4, h: H - 4, pattern: centres[take(2)] }], extra: [] };
    },
    /* four quarters, four figures */
    quarters: (take) => {
      const W = 9, H = 9, g = 0.7, s = (W - g) / 2;
      const pool = ['curls', 'water', 'zigzag', 'cloud', 'spiral', 'rosette', 'bars', 'meander'];
      const regions = [];
      for (let i = 0; i < 4; i++) regions.push({ x: (i % 2) * (s + g), y: (i >> 1) * (s + g), w: s, h: s, pattern: pool[take(3)] });
      return { W, H, regions, extra: [] };
    },
    /* two scales: a fine ground across the whole field, a bold figure over it */
    layered: (take) => {
      const W = 9, H = 9;
      const grounds = ['water', 'zigzag', 'dashes', 'bars'];
      const figures = ['rosette', 'spiral', 'curls', 'cloud'];
      return { W, H, regions: [{ x: 0, y: 0, w: W, h: H, pattern: grounds[take(2)], weight: 0.55 }, { x: 1.5, y: 1.5, w: W - 3, h: H - 3, pattern: figures[take(2)], weight: 1.25 }], extra: [] };
    },
    /* bands: the field cut into four bands, each its own figure */
    bands: (take) => {
      const W = 9, H = 9, hb = 2, gap = (H - 4 * hb) / 3;
      const pool = ['water', 'zigzag', 'curls', 'cloud', 'bars', 'dashes', 'spiral', 'meander'];
      const regions = [];
      for (let i = 0; i < 4; i++) regions.push({ x: 0, y: i * (hb + gap), w: W, h: hb, pattern: pool[take(3)] });
      return { W, H, regions, extra: [] };
    },
    /* the cloud script alone, as a field */
    cloudField: () => ({ W: 8, H: 8, regions: [{ x: 0, y: 0, w: 8, h: 8, pattern: 'cloud' }], extra: [] }),
    /* the talisman, with its parts chosen: head, body order, foot, spine, mirror, columns, border */
    talisman2: (take, t) => {
      t = Object.assign({ W: 6, H: 17, head: 'bars3', foot: 'knotSword', spine: false, mirror: false, columns: 1, core: false, bands: [4, 4], lexicon: ['cloud', 'zigzag', 'bars', 'curls', 'water', 'dashes', 'rosette', 'meander', 'ghost', 'eyes', 'well', 'hooks', 'loops', 'lock'], rope: false, dividers: true, bandH: [2, 3, 2, 3, 2, 3] }, t || {});
      const W = t.W, H = t.H;
      const regions = [], extra = [];
      const cx = W / 2;
      const bar = (y, x0, x1, weight) => extra.push({ pts: [[x0, y], [x1, y]], horiz: true, weight });
      let y = 0.4;
      /* the head */
      switch (t.head) {
        case 'bars3': bar(0.4, 0.4, W - 0.4); bar(1.35, 1.2, W - 1.2); bar(2.3, 0.4, W - 0.4); y = 3.2; break;
        case 'dots3': for (const [dx, dy] of [[0, 0.5], [-1.1, 1.6], [1.1, 1.6]]) extra.push({ pts: [[cx + dx - 0.22, dy], [cx + dx + 0.22, dy + 0.12]], brush: { curve: 'corner', entry: 'round', press0: 0.35, exit: 'blunt' }, weight: 1.3 }); y = 2.6; break;
        case 'crown': for (const dx of [-1.6, 0, 1.6]) extra.push({ pts: [[cx + dx, 2.4], [cx + dx, 0.9], [cx + dx + (dx < 0 ? -0.5 : dx > 0 ? 0.5 : 0.45), 0.35]], brush: { curve: 'corner', r: 0.25, over: 0.2, entry: 'round', press0: 0.2, exit: 'taper', tail: 0.6 } }); y = 3.0; break;
        case 'thunder': extra.push({ pts: [[cx - 0.2, 0.3], [cx + 0.2, 0.42]], brush: { curve: 'corner', entry: 'round', press0: 0.35, exit: 'blunt' }, weight: 1.3 }); bar(1.1, 0.6, W - 0.6); bar(1.9, 0.6, W - 0.6); extra.push({ pts: [[cx, 0.9], [cx, 2.5]], horiz: false }); y = 3.1; break;
        case 'roof': extra.push({ pts: [[cx - 0.2, 0.3], [cx + 0.2, 0.42]], brush: { curve: 'corner', entry: 'round', press0: 0.35, exit: 'blunt' }, weight: 1.3 }); extra.push({ pts: [[0.5, 2.2], [0.8, 1.2], [cx, 0.9], [W - 0.8, 1.2], [W - 0.5, 2.2]], brush: { curve: 'spline', tension: 5, entry: 'round', press0: 0.15, exit: 'taper', tail: 0.8 } }); y = 3.0; break;
        default: y = 0.6;
      }
      const bodyTop = y;
      /* the body */
      const nb = t.bands[0] + (t.bands[1] > t.bands[0] ? take(1) * (t.bands[1] - t.bands[0]) : 0);
      const footH = t.foot === 'sword' ? 2.6 : t.foot === 'tails' ? 2.8 : 3.4;
      const avail = H - y - footH;
      const hs = [];
      for (let i = 0; i < nb; i++) hs.push(t.bandH[(i + take(1)) % t.bandH.length]);
      const gapN = nb - 1;
      const scale = (avail - gapN * 0.8) / hs.reduce((a, b) => a + b, 0);
      for (let i = 0; i < nb; i++) {
        const h = Math.max(1.2, hs[i] * scale);
        const isCore = t.core && i === Math.floor(nb / 2);
        if (t.columns === 2 && !isCore) {
          const cw = (W - 1 - 0.9) / 2;
          regions.push({ x: 0.5, y, w: cw, h, choose: t.lexicon });
          regions.push({ x: 0.5 + cw + 0.9, y, w: cw, h, choose: t.lexicon });
        } else regions.push({ x: 0.5, y, w: W - 1, h, choose: isCore ? ['ghost', 'loops', 'lock', 'eyes'] : t.lexicon, mirror: t.mirror, weight: isCore ? 1.35 : 1 });
        y += h + 0.45;
        if (i < nb - 1) { if (t.dividers) bar(y - 0.1, 0.9, W - 0.9); y += 0.35; }
      }
      const bodyBottom = y;
      /* the foot */
      const knot = (kx, ky, R, turns, dir) => { const pts = []; for (let i = 0; i <= 30; i++) { const tt = i / 30, a = dir * tt * turns * 2 * Math.PI, rad = R * (1 - 0.9 * tt); pts.push([kx + rad * Math.cos(a), ky + rad * Math.sin(a)]); } return { pts, brush: { curve: 'spline', tension: 6, entry: 'round', press0: 0.1, tail: 1.6 } }; };
      const sword = (x0, y0, y1, hook) => ({ pts: hook ? [[x0, y0], [x0, y1], [x0 - 0.5, y1 - 0.45]] : [[x0, y0], [x0, y1]], brush: { curve: 'corner', r: 0.15, over: 0, entry: 'round', press0: 0.3, tail: 1.1, exit: 'taper' } });
      switch (t.foot) {
        case 'knotSword': extra.push(knot(cx, y + 1.05, 1.05, 2.2, take(1) ? 1 : -1)); extra.push(sword(cx, y + 2.3, H - 0.3, true)); break;
        case 'doubleKnot': extra.push(knot(cx - 1.2, y + 0.9, 0.85, 2, 1)); extra.push(knot(cx + 1.2, y + 0.9, 0.85, 2, -1)); extra.push(sword(cx, y + 1.9, H - 0.3, take(1) === 1)); break;
        case 'tails': for (const [dx, len] of [[-1.5, 1.6], [0, 2.4], [1.5, 2.0]]) extra.push({ pts: [[cx + dx, y + 0.2], [cx + dx + (dx < 0 ? -0.3 : dx > 0 ? 0.3 : 0), y + 0.2 + len]], brush: { curve: 'corner', entry: 'round', press0: 0.3, exit: 'taper', tail: len * 0.8 } }); break;
        case 'lockSword': regions.push({ x: cx - 1.3, y: y + 0.1, w: 2.6, h: 1.6, pattern: 'lock' }); extra.push(sword(cx, y + 1.9, H - 0.3, true)); break;
        case 'sword': extra.push(sword(cx, y + 0.2, H - 0.3, true)); break;
        default:
      }
      /* the spine: one line from head to foot, under everything */
      if (t.spine) extra.unshift({ pts: [[cx, bodyTop - 0.5], [cx, bodyBottom + 0.3]], brush: { curve: 'corner', entry: 'round', press0: 0.2, exit: 'taper', tail: 1.2, pulse: 0 }, weight: 0.85 });
      /* the rope border: two wavy lines inside the edge */
      if (t.rope) {
        const wave = (x0, y0, x1, y1, amp, ph) => { const pts = [], n = Math.round(Math.hypot(x1 - x0, y1 - y0) * 2), dx = (x1 - x0) / n, dy = (y1 - y0) / n, nx = -dy, ny = dx; for (let i = 0; i <= n; i++) { const s = ((i + ph) % 2 ? amp : -amp); pts.push([x0 + dx * i + nx * s * 2, y0 + dy * i + ny * s * 2]); } return { pts, brush: { curve: 'spline', tension: 5, entry: 'point', exit: 'taper', tail: 0.8, pulse: 0, bulge: 0, even: true, hmul: 1 }, weight: 0.7 }; };
        const m = -0.55;
        extra.push(wave(m, m, W - m, m, 0.09, 0), wave(W - m, m, W - m, H - m, 0.09, 1), wave(W - m, H - m, m, H - m, 0.09, 0), wave(m, H - m, m, m, 0.09, 1));
      }
      return { W, H, regions, extra };
    },
  };

  function compound(hex, size, o) {
    o = Object.assign({
      layout: 'talisman', style: 'zhu', frame: 'single', framew: 2.2, pad: 1.4, ratio: 0.6, w: 0.8, pulse: 0.22, bulge: 0.14,
      ink: '#c2452d', ground: '#15120f', id: 'c', tilt: 0, shape: 'square', tail: 1.2, small: false,
    }, o || {});
    const all = S2.bits(hex, 256);
    let cur = 0;
    const take = (k) => { let v = 0; for (let i = 0; i < k; i++) v = (v << 1) | all[cur++ % 256]; return v; };
    const L = LAYOUTS[o.layout](take, o.tal);
    const u = 1, w0 = o.ratio, p = u + w0;
    const wf = o.style === 'bai' || o.style === 'ink' || o.frame === 'none' ? 0 : w0 * o.framew;
    const pad = (o.tal && o.tal.rope ? o.pad + 1 : o.pad) * u;
    const W = 2 * wf + 2 * pad + L.W * p + w0, H = 2 * wf + 2 * pad + L.H * p + w0;
    const ox = wf + pad + w0 / 2, oy = wf + pad + w0 / 2;
    const col = o.style === 'bai' ? o.ground : o.ink;
    let shapes = '';
    const drawStrokes = (strokes, brushSpec, X, Y, weight) => {
      let out = '';
      strokes.forEach((st, k) => {
        const P = st.pts.map(([x, y]) => [X(x), Y(y)]);
        const sp = Object.assign({}, brushSpec, st.brush || {});
        const base = {
          w: w0 * o.w * (weight || 1) * (st.weight || 1), r: (sp.r == null ? 0.3 : sp.r) * p, over: sp.over == null ? 0.3 : sp.over, entry: sp.entry || 'point', entryLen: 0.6 * p, press0: sp.press0 == null ? 0.08 : sp.press0,
          exit: sp.exit || 'taper', tail: (sp.tail == null ? o.tail : sp.tail) * p, bulge: sp.even ? 0 : sp.bulge == null ? o.bulge : sp.bulge, bulgeSigma: sp.bulgeSigma, hmul: sp.hmul == null ? (sp.even ? 1 : 0.86) : sp.hmul, thin: 0,
          pulse: sp.even ? 0 : sp.pulse == null ? o.pulse : sp.pulse, mod: 0, unit: p, seed: 1, curve: sp.curve || 'corner', tension: sp.tension || 4, tremor: null, step: p / 4, spm: 5, qm: 6,
        };
        if (st.horiz === true) Object.assign(base, { entry: 'point', entryLen: 0.4 * p, exit: 'blunt', pulse: 0, curve: 'corner' });
        if (st.horiz === false) Object.assign(base, { entry: 'round', press0: 0.2, exit: 'taper', tail: 0.7 * p, pulse: 0, curve: 'corner' });
        const r = stroke(P, base);
        out += shape(r, col);
        void k;
      });
      /* one decimal is enough at these sizes; it halves the file */
      return out.replace(/(\d+\.\d)\d+/g, '$1');
    };
    const chosen = [];
    for (const R of L.regions) {
      let name = R.pattern;
      if (!name) {
        /* never the same figure twice in a row: the next in the list instead */
        let idx = take(Math.ceil(Math.log2(R.choose.length))) % R.choose.length;
        if (chosen.length && R.choose[idx] === chosen[chosen.length - 1]) idx = (idx + 1) % R.choose.length;
        name = R.choose[idx];
      }
      chosen.push(name);
      const pat = REGION[name];
      const rw = Math.max(1, Math.round(R.w)), rh = Math.max(1, Math.round(R.h));
      const nb = pat.bits(rw, rh);
      const b = [];
      for (let i = 0; i < nb; i++) b.push(all[cur++ % 256]);
      if (R.mirror) {
        /* the left half decides, the right half mirrors it */
        const hw = Math.max(1, Math.floor(rw / 2));
        const nb2 = pat.bits(hw, rh);
        const b2 = []; for (let i = 0; i < nb2; i++) b2.push(all[cur++ % 256]);
        let strokes = pat.make(b2, hw, rh);
        const mirrored = strokes.map((s) => Object.assign({}, s, { pts: s.pts.map(([x, y]) => [rw - x, y]) }));
        strokes = strokes.concat(mirrored);
        if (!pat.brush.even) strokes = orderStrokes(strokes);
        const sx = R.w / rw, sy = R.h / rh;
        shapes += drawStrokes(strokes, pat.brush, (x) => ox + (R.x + x * sx) * p, (y) => oy + (R.y + y * sy) * p, R.weight);
        continue;
      }
      const sx = R.w / rw, sy = R.h / rh;
      const X = (x) => ox + (R.x + x * sx) * p, Y = (y) => oy + (R.y + y * sy) * p;
      let strokes = pat.make(b, rw, rh);
      if (!pat.brush.even) strokes = orderStrokes(strokes);
      shapes += drawStrokes(strokes, pat.brush, X, Y, R.weight);
    }
    if (L.extra.length) shapes += drawStrokes(L.extra, { curve: 'corner', r: 0.2, over: 0.1, entry: 'point', exit: 'taper' }, (x) => ox + x * p, (y) => oy + y * p, 1);
    const bitsUsed = cur;
    let frame = '', clip = '';
    const cid = `cclip-${o.id}-${hex.slice(2, 8)}`;
    if (o.shape === 'round') {
      const Rr = Math.min(W, H) / 2, cx = W / 2, cy = H / 2;
      clip = `<clipPath id="${cid}"><circle cx="${f2(cx)}" cy="${f2(cy)}" r="${f2(Rr - wf - 0.5 * u)}"/></clipPath>`;
      shapes = `<g clip-path="url(#${cid})">${shapes}</g>`;
      if (o.style === 'zhu') frame = `<circle cx="${f2(cx)}" cy="${f2(cy)}" r="${f2(Rr - wf / 2)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    } else if (o.style === 'zhu' && o.frame !== 'none') {
      frame = `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.7 * u)}" fill="none" stroke="${o.ink}" stroke-width="${f2(wf)}"/>`;
    }
    const field = o.style !== 'bai' ? '' : o.shape === 'round' ? `<circle cx="${f2(W / 2)}" cy="${f2(H / 2)}" r="${f2(Math.min(W, H) / 2)}" fill="${o.ink}"/>` : `<rect x="0" y="0" width="${f2(W)}" height="${f2(H)}" rx="${f2(0.4 * u)}" fill="${o.ink}"/>`;
    const scale = size / Math.max(W, H);
    const pxW = Math.round(W * scale), pxH = Math.round(H * scale);
    const svg = `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${f2(W)} ${f2(H)}" role="img" aria-label="Hash seal ${hex.slice(0, 10)}"${o.tilt ? ` style="transform: rotate(${o.tilt}deg)"` : ''}>${clip}${field}${frame}${shapes}</svg>`;
    return o.meta ? { svg, bitsUsed, chosen } : svg;
  }

  return { stroke, shape, thread, dry, treeStrokes, stitchStrokes, orderStrokes, chainSegs, HANDS, SONG, PATTERNS, REGION, REGION_NAMES, LAYOUTS, render, seal, ornament, compound, rng, sample };
});
