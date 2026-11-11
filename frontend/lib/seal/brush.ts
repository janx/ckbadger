/**
 * The brush of the seals: a polyline becomes a filled shape with the width of a loaded
 * brush — an entry (a hidden, round tip or an exposed point), a press at the turns, a lift at the
 * end (the tip drawn out) or a blunt stop, horizontals a little thinner than verticals.
 *
 * A port of the parts of the design prototype's `brush.js` (2026-11-10 rubbing × Song print,
 * `docs/superpowers/design/2026-11-10-rubbing-song/src/brush.js`) that the seals use: `sample`,
 * `stroke`, `shape`, `chainSegs` (with seal3's `paths`), `orderStrokes`. Every expression keeps
 * the prototype's order of operations, so the same numbers come out; the fidelity test
 * (`__tests__/lib/seal-fidelity.test.ts`) holds the port to the prototype.
 *
 * Not ported, because no seal reaches it: the trembling line (tremor), per-unit pressure
 * (press / lift), mid-stroke thinning (thin), the random "life" of the width (mod, and with it
 * the prototype's `rng` — its phase only ever multiplies `mod`, which the seal sets to 0), and the
 * knot and point exits of the Song hands.
 */

export type Pt = readonly [number, number];

export type Curve = 'corner' | 'spline';
export type Entry = 'round' | 'point';
export type Exit = 'taper' | 'blunt';

/** Two decimals, as the prototype's `f2` writes them (`Number#toString` of the rounded value). */
export function f2(v: number): string {
  return (Math.round(v * 100) / 100).toString();
}

/** One decimal, as the prototype's `f1` writes it. */
export function f1(v: number): string {
  return (Math.round(v * 10) / 10).toString();
}

const clamp = (v: number, a: number, b: number): number => Math.max(a, Math.min(b, v));

export interface SampleOptions {
  readonly curve: Curve;
  /** Spline tension: the control points sit 1 / tension of the neighbours' span away. */
  readonly tension: number;
  /** Samples per spline segment. */
  readonly spm: number;
  /** Samples per corner quadratic. */
  readonly qm: number;
  /** Sample spacing along a straight piece. */
  readonly step: number;
  /** Corner radius. */
  readonly r: number;
  /** 0 cuts the corner, 1 swings round it. */
  readonly over: number;
}

export interface SamplePoint {
  x: number;
  y: number;
  /** Arc length from the start. */
  s: number;
  tx: number;
  ty: number;
}

export interface Sampled {
  readonly pts: SamplePoint[];
  /** Arc lengths of the turns, where the brush presses. */
  readonly cornerS: number[];
  /** The whole length. */
  readonly L: number;
}

/**
 * Polyline → pieces (lines, quadratics at the corners; or one spline through every point) →
 * dense samples with arc length and tangent.
 */
export function sample(P: readonly Pt[], o: SampleOptions): Sampled {
  if (P.length < 2) throw new Error(`seal: a stroke needs two points, got ${P.length}`);
  const pts: SamplePoint[] = [];
  const cornerS: number[] = [];
  let s = 0;
  const push = (x: number, y: number) => {
    if (pts.length) {
      const l = pts[pts.length - 1];
      s += Math.hypot(x - l.x, y - l.y);
    }
    pts.push({ x, y, s, tx: 0, ty: 0 });
  };
  if (o.curve === 'spline' && P.length > 2) {
    const k = o.tension;
    push(P[0][0], P[0][1]);
    for (let i = 0; i < P.length - 1; i++) {
      const p0 = P[Math.max(0, i - 1)],
        p1 = P[i],
        p2 = P[i + 1],
        p3 = P[Math.min(P.length - 1, i + 2)];
      const c1 = [p1[0] + (p2[0] - p0[0]) / k, p1[1] + (p2[1] - p0[1]) / k];
      const c2 = [p2[0] - (p3[0] - p1[0]) / k, p2[1] - (p3[1] - p1[1]) / k];
      const m = o.spm;
      for (let j = 1; j <= m; j++) {
        const t = j / m,
          u = 1 - t;
        push(
          u * u * u * p1[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t * t * t * p2[0],
          u * u * u * p1[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t * t * t * p2[1]
        );
      }
      if (i + 2 < P.length) {
        const b = [p3[0] - p2[0], p3[1] - p2[1]];
        const din = [p2[0] - p1[0], p2[1] - p1[1]];
        if (Math.abs(din[0] * b[1] - din[1] * b[0]) > 1e-6) cornerS.push(s);
      }
    }
  } else {
    type Piece = { t: 'L'; a: Pt; b: Pt } | { t: 'Q'; a: Pt; c: Pt; b: Pt };
    const pieces: Piece[] = [];
    let cur: Pt = P[0];
    for (let i = 1; i < P.length - 1; i++) {
      const A = P[i - 1],
        C = P[i],
        B = P[i + 1];
      const vin = [C[0] - A[0], C[1] - A[1]],
        vout = [B[0] - C[0], B[1] - C[1]];
      const lin = Math.hypot(vin[0], vin[1]),
        lout = Math.hypot(vout[0], vout[1]);
      if (lin < 1e-9 || lout < 1e-9) continue;
      const din = [vin[0] / lin, vin[1] / lin],
        dout = [vout[0] / lout, vout[1] / lout];
      const cross = din[0] * dout[1] - din[1] * dout[0],
        dot = din[0] * dout[0] + din[1] * dout[1];
      if (Math.abs(cross) < 1e-6 && dot > 0) continue;
      const r = Math.min(o.r, lin / 2, lout / 2);
      const p1: Pt = [C[0] - din[0] * r, C[1] - din[1] * r],
        p2: Pt = [C[0] + dout[0] * r, C[1] + dout[1] * r];
      const mid = [(p1[0] + p2[0]) / 2, (p1[1] + p2[1]) / 2];
      const ctl: Pt = [C[0] + (C[0] - mid[0]) * o.over, C[1] + (C[1] - mid[1]) * o.over];
      pieces.push({ t: 'L', a: cur, b: p1 });
      pieces.push({ t: 'Q', a: p1, c: ctl, b: p2 });
      cur = p2;
    }
    pieces.push({ t: 'L', a: cur, b: P[P.length - 1] });
    for (const pc of pieces) {
      if (pc.t === 'L') {
        const len = Math.hypot(pc.b[0] - pc.a[0], pc.b[1] - pc.a[1]);
        if (!pts.length) push(pc.a[0], pc.a[1]);
        if (len < 1e-9) continue;
        const m = Math.max(1, Math.ceil(len / o.step));
        for (let i = 1; i <= m; i++) {
          const t = i / m;
          push(pc.a[0] + (pc.b[0] - pc.a[0]) * t, pc.a[1] + (pc.b[1] - pc.a[1]) * t);
        }
      } else {
        const m = o.qm;
        for (let i = 1; i <= m; i++) {
          const t = i / m,
            u = 1 - t;
          push(
            u * u * pc.a[0] + 2 * u * t * pc.c[0] + t * t * pc.b[0],
            u * u * pc.a[1] + 2 * u * t * pc.c[1] + t * t * pc.b[1]
          );
          if (i === Math.ceil(m / 2)) cornerS.push(s);
        }
      }
    }
  }
  for (let i = 0; i < pts.length; i++) {
    const a = pts[Math.max(0, i - 1)],
      b = pts[Math.min(pts.length - 1, i + 1)];
    const tx = b.x - a.x,
      ty = b.y - a.y;
    // Two samples can coincide (a spline doubling back on itself); the prototype then gives the
    // point a zero tangent, which pinches the outline there. Kept: it is the design's line.
    const l = Math.hypot(tx, ty) || 1;
    pts[i].tx = tx / l;
    pts[i].ty = ty / l;
  }
  return { pts, cornerS, L: s };
}

/** One stroke's brush, fully resolved (the seal renderer fills every field). */
export interface StrokeOptions extends SampleOptions {
  /** Base width. */
  readonly w: number;
  readonly entry: Entry;
  readonly entryLen: number;
  /** The press just after the entry. */
  readonly press0: number;
  readonly exit: Exit;
  /** Length of the lift at a tapered exit. */
  readonly tail: number;
  /** The press at a turn, and its spread in units (0.45 when absent, as the prototype). */
  readonly bulge: number;
  readonly bulgeSigma: number | undefined;
  /** Horizontal width over vertical width. */
  readonly hmul: number;
  /** The pulse of pressure at every lattice unit. */
  readonly pulse: number;
  /** The lattice unit the lengths above are measured in. */
  readonly unit: number;
}

/** A blot of ink where the brush landed or stopped round: [cx, cy, r]. */
export type Cap = readonly [number, number, number];

export interface Outline {
  /** The outline polygon: the left edge forward, then the right edge back. */
  readonly polygon: Pt[];
  readonly caps: Cap[];
  readonly L: number;
}

/** One brush stroke along `P`: its outline and the round caps where the tip stays round. */
export function stroke(P: readonly Pt[], o: StrokeOptions): Outline {
  const u = o.unit;
  const { pts, cornerS, L } = sample(P, o);
  const exit = o.exit;
  const sig = (o.bulgeSigma || 0.45) * u;
  const widths = pts.map((p) => {
    let w = o.w * (o.hmul + (1 - o.hmul) * Math.abs(p.ty));
    const ent = o.entryLen;
    if (o.entry === 'point') {
      w *= Math.pow(clamp(p.s / ent, 0, 1), 0.55);
      if (o.press0) w *= 1 + o.press0 * Math.exp(-Math.pow((p.s - ent * 1.1) / (0.3 * u), 2));
    } else w *= 1 + o.press0 * Math.exp(-Math.pow((p.s - 0.3 * u) / (0.32 * u), 2));
    const rem = L - p.s;
    if (exit === 'taper') w *= Math.pow(clamp(rem / o.tail, 0, 1), 0.75);
    else w *= 1 + 0.2 * Math.exp(-Math.pow((rem - 0.18 * u) / (0.26 * u), 2));
    for (const sc of cornerS) w *= 1 + o.bulge * Math.exp(-Math.pow((p.s - sc) / sig, 2));
    if (o.pulse) w *= 1 - o.pulse * Math.pow(Math.sin((Math.PI * p.s) / u), 2);
    // The brush never draws thinner than 3 % of its width: the prototype's own floor.
    return Math.max(w, 0.03 * o.w);
  });
  const left: Pt[] = [],
    right: Pt[] = [];
  pts.forEach((p, i) => {
    const nx = -p.ty,
      ny = p.tx,
      h = widths[i] / 2;
    left.push([p.x + nx * h, p.y + ny * h]);
    right.push([p.x - nx * h, p.y - ny * h]);
  });
  const polygon = left.concat(right.reverse());
  const caps: Cap[] = [];
  if (o.entry !== 'point') caps.push([pts[0].x, pts[0].y, widths[0] / 2]);
  if (exit === 'blunt')
    caps.push([pts[pts.length - 1].x, pts[pts.length - 1].y, widths[widths.length - 1] / 2]);
  return { polygon, caps, L };
}

/**
 * The prototype's markup of a stroke: one filled path through the outline, a circle per cap,
 * two decimals. `paint` is a validated colour (see `assertPaint`).
 */
export function shape(r: Outline, paint: string): string {
  const [first, ...rest] = r.polygon;
  let d = `M${f2(first[0])} ${f2(first[1])}`;
  for (const q of rest) d += `L${f2(q[0])} ${f2(q[1])}`;
  d += 'Z';
  return (
    `<path d="${d}" fill="${paint}"/>` +
    r.caps
      .map((c) => `<circle cx="${f2(c[0])}" cy="${f2(c[1])}" r="${f2(c[2])}" fill="${paint}"/>`)
      .join('')
  );
}

/** A unit segment on a lattice: [x1, y1, x2, y2]. */
export type Seg = readonly [number, number, number, number];

/** Unit segments → polylines of lattice points, a straight continuation preferred (seal3 `paths`). */
export function segmentPaths(segs: readonly Seg[]): Pt[][] {
  const key = (x: number, y: number) => x + ',' + y;
  const adj = new Map<string, { id: number; to: Pt }[]>();
  const edges: { used: boolean }[] = [];
  for (const [x1, y1, x2, y2] of segs) {
    const id = edges.length;
    edges.push({ used: false });
    const ends: [Pt, Pt][] = [
      [
        [x1, y1],
        [x2, y2],
      ],
      [
        [x2, y2],
        [x1, y1],
      ],
    ];
    for (const [p, q] of ends) {
      const k = key(p[0], p[1]);
      let list = adj.get(k);
      if (!list) {
        list = [];
        adj.set(k, list);
      }
      list.push({ id, to: q });
    }
  }
  const at = (k: string) => {
    const list = adj.get(k);
    if (!list) throw new Error(`seal: no lattice point ${k}`);
    return list;
  };
  const deg = (k: string) => at(k).filter((e) => !edges[e.id].used).length;
  const out: Pt[][] = [];
  const starts = [...adj.keys()].sort(
    (a, b) => (at(a).length % 2 === 1 ? 0 : 1) - (at(b).length % 2 === 1 ? 0 : 1)
  );
  for (const s of starts) {
    while (deg(s) > 0) {
      const [sx, sy] = s.split(',').map(Number);
      const pts: Pt[] = [[sx, sy]];
      let cur = s;
      let dir: Pt | null = null;
      for (;;) {
        const opts = at(cur).filter((e) => !edges[e.id].used);
        if (!opts.length) break;
        let pick = opts[0];
        if (dir) {
          const d = dir;
          const last = pts[pts.length - 1];
          const straight = opts.find(
            (e) => e.to[0] - last[0] === d[0] && e.to[1] - last[1] === d[1]
          );
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

/** Unit segments of any generator → polylines, oriented down / right as a brush goes. */
export function chainSegs(segs: readonly Seg[]): { pts: Pt[] }[] {
  return segmentPaths(segs).map((pts) => {
    const a = pts[0],
      b = pts[pts.length - 1];
    return { pts: b[0] + b[1] < a[0] + a[1] ? pts.slice().reverse() : pts };
  });
}

/** Writing order: greedy, from the top left, preferring the next stroke down and to the right. */
export function orderStrokes<T extends { readonly pts: readonly Pt[] }>(
  strokes: readonly T[]
): T[] {
  if (strokes.length < 2) return strokes.slice();
  const rem = strokes.slice();
  rem.sort((a, b) => a.pts[0][1] + 0.5 * a.pts[0][0] - (b.pts[0][1] + 0.5 * b.pts[0][0]));
  let cur = rem.shift() as T;
  const out = [cur];
  while (rem.length) {
    const e = cur.pts[cur.pts.length - 1];
    let bi = 0,
      bc = Infinity;
    rem.forEach((s, i) => {
      const st = s.pts[0],
        dx = st[0] - e[0],
        dy = st[1] - e[1];
      const c = Math.hypot(dx, dy) + 0.7 * Math.max(0, -dy) + 0.35 * Math.max(0, -dx);
      if (c < bc) {
        bc = c;
        bi = i;
      }
    });
    cur = rem.splice(bi, 1)[0];
    out.push(cur);
  }
  return out;
}
