import type { PipelineProposalState } from '@/lib/api';
import {
  STAGE_BASE,
  chipPaint,
  diamondPath,
  mixColor,
  paintFromBase,
  tok,
  type ShadeStep,
  type SquareStyle,
} from '@/lib/pipeline/encoding';
import {
  chainPaths,
  rulerPath,
  windowLabel,
  windowMarksPath,
  windowRuler,
  type ChainSlotState,
  type RulerSlot,
  type Scene,
  type SceneSquare,
} from '@/lib/pipeline/layout';
import type { Transition, TransitionEvent } from '@/lib/pipeline/transitions';

/**
 * The pipeline animator, ported from the prototype's `go` / `stepNode` / `exitNode` / `ripple`. It
 * writes SVG attributes directly from one requestAnimationFrame loop that only runs while something
 * moves; React never re-renders per frame. With reduced motion every call jumps straight to its
 * final state.
 */

const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);

export const smoothstep = (a: number, b: number, x: number) => {
  const t = clamp01((x - a) / (b - a));
  return t * t * (3 - 2 * t);
};

/** CSS cubic-bezier(x1, y1, x2, y2) as a function of progress. */
export function bezier(x1: number, y1: number, x2: number, y2: number): (x: number) => number {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const sx = (t: number) => ((ax * t + bx) * t + cx) * t;
  const sy = (t: number) => ((ay * t + by) * t + cy) * t;
  const dx = (t: number) => (3 * ax * t + 2 * bx) * t + cx;
  return (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let t = x;
    for (let i = 0; i < 7; i++) {
      const d = dx(t);
      if (Math.abs(d) < 1e-6) break;
      t -= (sx(t) - x) / d;
    }
    return sy(clamp01(t));
  };
}

export const E = {
  enter: bezier(0.2, 0.82, 0.2, 1),
  fade: bezier(0.25, 0.1, 0.25, 1),
  fall: bezier(0.55, 0, 0.85, 0.65),
  lob: (t: number) => 0.5 - 0.5 * Math.cos(Math.PI * t),
};

/** Grows past full size by 8 %, then settles. */
export const birthEase = (u: number) =>
  u < 0.78 ? 1.08 * smoothstep(0, 1, u / 0.78) : 1.08 - 0.08 * smoothstep(0, 1, (u - 0.78) / 0.22);

export interface MotionClock {
  now(): number;
  frame(cb: (ts: number) => void): number;
  cancel(id: number): void;
}

export const browserClock: MotionClock = {
  now: () => performance.now(),
  frame: (cb) => requestAnimationFrame(cb),
  cancel: (id) => cancelAnimationFrame(id),
};

export type Values = Record<string, number>;
type Ease = (u: number) => number;

/** Colour state of a square: a base colour expression plus its encoding. */
export interface Look {
  base: string;
  style: SquareStyle;
  step: ShadeStep;
  cellbase: boolean;
}

interface Tween {
  t0: number;
  dur: number;
  from: Values;
  to: Values;
  keys: string[];
  ease: Ease;
  arc: boolean;
  birth: boolean;
  colorLate: boolean;
  kind: 'seat' | 'drop';
  fromLook: Look | null;
  toLook: Look | null;
  onEnd?: (n: MotionNode, instant: boolean) => void;
}

export interface MotionNode {
  id: string;
  el: SVGElement;
  /** Current values: X, Y, s, op for squares; X, Y, w, h, op, solid, fresh for cards. */
  v: Values;
  /** Where the last go() was heading, per key: a repeated target does not restart a tween. */
  target: Values;
  look: Look | null;
  paint: (n: MotionNode) => void;
  /** Running tweens by channel; channels animate independent keys at overlapping times. */
  tws: Map<string, Tween>;
  exiting: boolean;
  /** Last written colour key, so unchanged colours are not rewritten every frame. */
  painted: string;
}

export interface GoOptions {
  dur?: number;
  delay?: number;
  ease?: Ease;
  /** Lob along a quadratic arc above both ends (a commit flying into its block). */
  arc?: boolean;
  /** Grow from nothing with an overshoot (a new cellbase). */
  birth?: boolean;
  /** Keep the old colour until the last 30 % of the move. */
  colorLate?: boolean;
  /** Start values that replace the current ones right away (e.g. fade in from 0). */
  from?: Values;
  look?: Look;
  /** Independent tween slot (default "main"). */
  channel?: string;
  onEnd?: (n: MotionNode, instant: boolean) => void;
}

export type ExitKind = 'hide' | 'retire' | 'drop' | 'orphan';

/** A ripple in flight. */
interface Fx {
  el: SVGElement;
  t0: number;
  dur: number;
  r: number;
}

const NS = 'http://www.w3.org/2000/svg';

/** Paints a square node (rect, or the cellbase diamond) from its values and look. */
export function paintSquareNode(n: MotionNode): void {
  const { X, Y, op } = n.v;
  const s = Math.max(0.2, n.v.s);
  const tw = n.tws.get('main');
  const off = tw && tw.birth ? (tw.to.s - s) / 2 : 0;
  const el = n.el;
  const look = n.look;
  if (!look) throw new Error(`motion: square ${n.id} has no look`);
  if (look.cellbase) {
    el.setAttribute('d', diamondPath(X + off, Y + off, s));
  } else {
    el.setAttribute('x', (X + off).toFixed(2));
    el.setAttribute('y', (Y + off).toFixed(2));
    el.setAttribute('width', s.toFixed(2));
    el.setAttribute('height', s.toFixed(2));
    el.setAttribute('rx', Math.min(1.5, s * 0.2).toFixed(2));
  }
  // A square born from nothing stays unseen until it has a size (a 0-size diamond is still a dot).
  el.setAttribute('opacity', (n.v.s < 0.5 ? 0 : op).toFixed(3));
  const key = `${look.base}|${look.style}|${look.step}`;
  if (key === n.painted) return;
  n.painted = key;
  if (look.cellbase) {
    el.style.setProperty('fill', look.base);
    return;
  }
  const p = paintFromBase(look.base, look.style, look.step);
  el.style.setProperty('fill', p.fill);
  el.style.setProperty('stroke', p.stroke ?? 'none');
  if (p.fillOpacity === null) el.removeAttribute('fill-opacity');
  else el.setAttribute('fill-opacity', String(p.fillOpacity));
  if (p.stroke) el.setAttribute('stroke-width', '1');
  if (p.strokeDasharray) el.setAttribute('stroke-dasharray', p.strokeDasharray);
  else el.removeAttribute('stroke-dasharray');
}

export class Animator {
  private readonly active = new Set<MotionNode>();
  private fx: Fx[] = [];
  private timers: { t: number; fn: () => void }[] = [];
  private readonly hooks = new Set<(now: number) => void>();
  private frameId: number | null = null;

  constructor(
    private readonly clock: MotionClock,
    private reduced: boolean
  ) {}

  get isReduced(): boolean {
    return this.reduced;
  }

  /** Switching to reduced motion jumps everything in flight to its end. */
  setReduced(reduced: boolean): void {
    this.reduced = reduced;
    if (reduced) this.finishAll();
  }

  node(
    id: string,
    el: SVGElement,
    v: Values,
    look: Look | null,
    paint: (n: MotionNode) => void
  ): MotionNode {
    const n: MotionNode = {
      id,
      el,
      v: { ...v },
      target: { ...v },
      look,
      paint,
      tws: new Map(),
      exiting: false,
      painted: '',
    };
    paint(n);
    return n;
  }

  /** Whether go(n, to) would head somewhere new. */
  differs(n: MotionNode, to: Values, eps = 0.05): boolean {
    return Object.keys(to).some((k) => {
      const t = n.target[k];
      return t === undefined || Math.abs(t - to[k]) > eps;
    });
  }

  go(n: MotionNode, to: Values, o: GoOptions = {}): void {
    const keys = Object.keys(to);
    for (const k of keys) {
      if (n.v[k] === undefined) throw new Error(`motion: node ${n.id} has no value "${k}"`);
    }
    const toLook = o.look ?? null;
    const channel = o.channel ?? 'main';
    Object.assign(n.target, to);
    if (this.reduced) {
      for (const k of keys) n.v[k] = to[k];
      if (toLook) n.look = toLook;
      n.tws.delete(channel);
      if (!n.tws.size) this.active.delete(n);
      n.paint(n);
      o.onEnd?.(n, true);
      return;
    }
    if (o.from) Object.assign(n.v, o.from);
    const from: Values = {};
    for (const k of keys) from[k] = n.v[k];
    const fromLook = n.look;
    if (toLook && fromLook && !o.colorLate) n.look = { ...toLook, base: fromLook.base };
    n.tws.set(channel, {
      t0: this.clock.now() + (o.delay ?? 0),
      dur: o.dur ?? 260,
      from,
      to: { ...to },
      keys,
      ease: o.ease ?? E.enter,
      arc: !!o.arc,
      birth: !!o.birth,
      colorLate: !!o.colorLate,
      kind: 'seat',
      fromLook,
      toLook,
      onEnd: o.onEnd,
    });
    this.active.add(n);
    n.paint(n);
    this.kick();
  }

  /** Animate a node out and remove its element; `onGone` runs once it is removed. */
  exit(n: MotionNode, kind: ExitKind, delay = 0, onGone?: () => void): void {
    n.exiting = true;
    n.tws.clear();
    const done = () => {
      n.el.remove();
      n.tws.clear();
      this.active.delete(n);
      onGone?.();
    };
    if (this.reduced) {
      done();
      return;
    }
    if (kind === 'hide') this.go(n, { op: 0 }, { dur: 200, delay, ease: E.fade, onEnd: done });
    else if (kind === 'retire')
      this.go(n, { X: n.v.X + 18, op: 0 }, { dur: 600, delay, ease: E.fade, onEnd: done });
    else if (kind === 'orphan')
      this.go(n, { Y: n.v.Y + 12, op: 0 }, { dur: 600, delay, ease: E.fade, onEnd: done });
    else {
      n.tws.set('main', {
        t0: this.clock.now() + 60 + delay,
        dur: 900,
        from: { s: n.v.s },
        to: {},
        keys: [],
        ease: E.enter,
        arc: false,
        birth: false,
        colorLate: false,
        kind: 'drop',
        fromLook: n.look,
        toLook: null,
        onEnd: done,
      });
      this.active.add(n);
      this.kick();
    }
  }

  /** A ring that opens and fades: the mark of a discrete event. */
  ripple(layer: SVGElement, x: number, y: number, r: number, color: string, delay = 0): void {
    if (this.reduced) return;
    const el = document.createElementNS(NS, 'circle');
    el.setAttribute('cx', String(x));
    el.setAttribute('cy', String(y));
    el.setAttribute('r', '0');
    el.setAttribute('fill', 'none');
    el.setAttribute('stroke-width', '1');
    el.setAttribute('vector-effect', 'non-scaling-stroke');
    el.setAttribute('opacity', '0');
    el.style.setProperty('stroke', color);
    layer.appendChild(el);
    this.fx.push({ el, t0: this.clock.now() + delay, dur: 480, r });
    this.kick();
  }

  /** Run fn on the animation timeline (immediately with reduced motion). */
  after(delay: number, fn: () => void): void {
    if (this.reduced) {
      fn();
      return;
    }
    this.timers.push({ t: this.clock.now() + delay, fn });
    this.kick();
  }

  /** Called on every frame while anything moves (and once more when it stops). */
  addFrameHook(fn: (now: number) => void): () => void {
    this.hooks.add(fn);
    return () => {
      this.hooks.delete(fn);
    };
  }

  isBusy(): boolean {
    return this.active.size > 0 || this.fx.length > 0 || this.timers.length > 0;
  }

  /** Jump every tween, effect and scheduled callback to its end. */
  finishAll(): void {
    if (this.frameId !== null) {
      this.clock.cancel(this.frameId);
      this.frameId = null;
    }
    for (let guard = 0; this.isBusy(); guard++) {
      if (guard > 100) throw new Error('motion: work keeps scheduling more work while finishing');
      const now = this.clock.now();
      for (const n of [...this.active]) {
        for (const tw of n.tws.values()) tw.t0 = -Infinity;
        this.stepNode(n, now);
      }
      const fx = this.fx;
      this.fx = [];
      for (const f of fx) f.el.remove();
      const timers = this.timers.sort((a, b) => a.t - b.t);
      this.timers = [];
      for (const t of timers) t.fn();
    }
    const now = this.clock.now();
    for (const h of this.hooks) h(now);
  }

  dispose(): void {
    if (this.frameId !== null) this.clock.cancel(this.frameId);
    this.frameId = null;
    for (const f of this.fx) f.el.remove();
    this.fx = [];
    this.timers = [];
    this.active.clear();
    this.hooks.clear();
  }

  private kick(): void {
    if (this.frameId === null) this.frameId = this.clock.frame(this.tick);
  }

  private readonly tick = (): void => {
    this.frameId = null;
    const now = this.clock.now();
    const due = this.timers.filter((t) => t.t <= now).sort((a, b) => a.t - b.t);
    this.timers = this.timers.filter((t) => t.t > now);
    for (const t of due) t.fn();
    for (const n of [...this.active]) this.stepNode(n, now);
    this.stepFx(now);
    for (const h of this.hooks) h(now);
    if (this.isBusy()) this.kick();
  };

  private stepNode(n: MotionNode, now: number): void {
    if (!n.tws.size) {
      this.active.delete(n);
      return;
    }
    const finished: Tween[] = [];
    let moved = false;
    for (const [channel, tw] of [...n.tws]) {
      let u = (now - tw.t0) / tw.dur;
      if (u < 0) continue;
      if (u > 1) u = 1;
      moved = true;
      this.stepTween(n, tw, u);
      if (u === 1) {
        n.tws.delete(channel);
        if (tw.toLook) n.look = tw.toLook;
        finished.push(tw);
      }
    }
    if (!n.tws.size) this.active.delete(n);
    if (moved) n.paint(n);
    for (const tw of finished) tw.onEnd?.(n, false);
  }

  private stepTween(n: MotionNode, tw: Tween, u: number): void {
    if (tw.kind === 'drop') {
      const look = tw.fromLook as Look;
      n.look = { ...look, base: mixColor(look.base, tok('drop'), smoothstep(0, 0.6, u)) };
      n.v.s = tw.from.s * (1 - Math.pow(smoothstep(0.4, 1, u), 2));
      n.v.op = 1 - smoothstep(0.8, 1, u);
      return;
    }
    const e = tw.ease(u);
    const f = tw.from;
    const t = tw.to;
    for (const k of tw.keys) n.v[k] = f[k] + (t[k] - f[k]) * e;
    if (tw.birth) n.v.s = t.s * birthEase(u);
    if (tw.arc) {
      const d = Math.hypot(t.X - f.X, t.Y - f.Y);
      const cx = (f.X + t.X) / 2;
      const cy = Math.min(f.Y, t.Y) - 24 - 0.1 * d;
      const a = (1 - e) * (1 - e);
      const b = 2 * (1 - e) * e;
      const c = e * e;
      n.v.X = a * f.X + b * cx + c * t.X;
      n.v.Y = a * f.Y + b * cy + c * t.Y;
    }
    if (tw.toLook && tw.fromLook) {
      const k = tw.colorLate ? clamp01((u - 0.7) / 0.3) : e;
      const encoding = tw.colorLate ? tw.fromLook : tw.toLook;
      n.look = { ...encoding, base: mixColor(tw.fromLook.base, tw.toLook.base, k) };
    }
  }

  private stepFx(now: number): void {
    this.fx = this.fx.filter((f) => {
      const life = (now - f.t0) / f.dur;
      if (life < 0) return true;
      if (life >= 1) {
        f.el.remove();
        return false;
      }
      f.el.setAttribute('r', (f.r * (0.2 + 0.8 * (1 - Math.pow(1 - life, 3)))).toFixed(2));
      f.el.setAttribute('opacity', (Math.sin(Math.PI * life) * 0.9).toFixed(3));
      return true;
    });
  }
}

// ---------- Scene motion: React draws the target geometry, this animates the way there ----------

/** What the motion layer needs from the current view (hover state lives in React). */
export interface MotionView {
  highlighted(cardId: string): boolean;
  /** Height H whose proposal window is drawn. */
  anchor(): number;
}

/** One observed change: a new transition (new seq) or the landing → settled phase step. */
export interface MotionCtx {
  seq: number;
  transition: Transition;
}

/** Elements React is about to remove, cloned while they still show their animated state. */
export interface Departing {
  cards: Map<string, SVGGElement>;
  squares: Map<string, SVGElement>;
}

export const NEXT_CARD_ID = '__next';

interface CardRec {
  node: MotionNode;
  mined: boolean;
  number: number;
  hash: string;
  parentHash: string;
}

interface SquareRec {
  node: MotionNode;
  /** 'pool' or the block hash of the card it sits in (its values are local to that card). */
  container: string;
}

const STRIP = ['data-card', 'data-next', 'data-sq', 'data-chip', 'data-number', 'data-slot'];

function strip(el: Element): void {
  for (const a of STRIP) {
    el.removeAttribute(a);
    el.querySelectorAll(`[${a}]`).forEach((x) => x.removeAttribute(a));
  }
}

const lookOf = (q: SceneSquare): Look => ({
  base: STAGE_BASE[q.stage],
  style: q.style,
  step: q.step,
  cellbase: q.kind === 'cellbase',
});

const sameLook = (a: Look | null, b: Look) =>
  !!a && a.base === b.base && a.style === b.style && a.step === b.step;

/** Exponential decay with a 600 ms time constant, landing exactly on 0 after 2.4 s. */
export const FRESH_MS = 2400;
const freshEase = (u: number) => (1 - Math.exp(-4 * u)) / (1 - Math.exp(-4));

function paintChip(el: Element, state: PipelineProposalState): void {
  const p = chipPaint(state);
  (el as SVGElement).style.setProperty('fill', p.fill);
  (el as SVGElement).style.setProperty('stroke', p.stroke ?? 'none');
  el.setAttribute('fill-opacity', String(p.fillOpacity));
  if (p.stroke) el.setAttribute('stroke-width', '1');
}

function layer(svg: SVGSVGElement, name: string): SVGGElement {
  const el = svg.querySelector(name === 'pool' ? '[data-pool]' : `[data-layer="${name}"]`);
  if (!el) throw new Error(`motion: the pipeline SVG has no ${name} layer`);
  return el as SVGGElement;
}

/**
 * Drives the animator from scene diffs. After every React commit it compares what each node shows
 * now (its records) with the new target geometry and the observed transition events, then animates
 * the difference. Departed elements come in as clones and are animated out in an exit layer.
 */
export class SceneMotion {
  private readonly cards = new Map<string, CardRec>();
  private readonly exitingCards = new Set<CardRec>();
  private readonly squares = new Map<string, SquareRec>();
  private scene: Scene | null = null;
  private svg: SVGSVGElement | null = null;
  private seq = -1;
  private readonly unhook: () => void;

  constructor(
    private readonly animator: Animator,
    private readonly view: MotionView
  ) {
    this.unhook = animator.addFrameHook(() => this.paintChain());
  }

  setReduced(reduced: boolean): void {
    this.animator.setReduced(reduced);
  }

  dispose(): void {
    this.unhook();
    this.animator.dispose();
    for (const c of this.exitingCards) c.node.el.remove();
    this.exitingCards.clear();
    this.cards.clear();
    this.squares.clear();
  }

  /** Called before React commits `next`: clone what is about to disappear. */
  captureDeparting(svg: SVGSVGElement, prev: Scene, next: Scene): Departing {
    const out: Departing = { cards: new Map(), squares: new Map() };
    const keepCards = new Set(next.cards.map((c) => c.id));
    const cardsLayer = layer(svg, 'cards');
    const gone = new Set<string>();
    for (const c of prev.cards) {
      if (keepCards.has(c.id)) continue;
      gone.add(c.id);
      const el = cardsLayer.querySelector(`[data-card="${c.id}"]`);
      if (el) out.cards.set(c.id, el.cloneNode(true) as SVGGElement);
    }
    // NEXT turns into the landing block in place, so it only fades when nothing lands there.
    if (prev.next && !next.next && next.phase !== 'landing') {
      const el = cardsLayer.querySelector('[data-next]');
      if (el) out.cards.set(NEXT_CARD_ID, el.cloneNode(true) as SVGGElement);
    }
    const keep = new Set<string>();
    if (next.pool)
      for (const q of [...next.pool.pending.squares, ...next.pool.proposed.squares]) keep.add(q.id);
    for (const c of next.cards) for (const q of c.squares) keep.add(q.id);
    const prevSquares: SceneSquare[] = [
      ...(prev.pool ? [...prev.pool.pending.squares, ...prev.pool.proposed.squares] : []),
      ...prev.cards.filter((c) => !gone.has(c.id)).flatMap((c) => c.squares),
    ];
    for (const q of prevSquares) {
      if (keep.has(q.id)) continue;
      const el = svg.querySelector(`[data-sq="${q.id}"]`);
      if (el) out.squares.set(q.id, el.cloneNode(true) as SVGElement);
    }
    return out;
  }

  /** Bring every node from what it shows now to `scene`; `ctx` carries the observed events. */
  sync(
    svg: SVGSVGElement,
    prev: Scene | null,
    scene: Scene,
    ctx: MotionCtx,
    departing: Departing
  ): void {
    this.svg = svg;
    this.scene = scene;
    const fresh = ctx.seq !== this.seq;
    this.seq = ctx.seq;
    const tr = ctx.transition;
    const boot = !prev || (fresh && tr.mode === 'boot');
    const events: TransitionEvent[] = fresh ? tr.events : [];
    const phaseStep = !!prev && prev.phase !== scene.phase;
    const settle = phaseStep || (fresh && tr.mode === 'settle');
    const mined = events.find(
      (e): e is Extract<TransitionEvent, { kind: 'mined' }> => e.kind === 'mined'
    );
    const txEvents = new Map<string, TransitionEvent>();
    const cardEvents = new Map<string, TransitionEvent>();
    for (const e of events) {
      if ('hash' in e) txEvents.set(e.hash, e);
      else if (e.kind === 'retire' || e.kind === 'orphan' || e.kind === 'enter')
        cardEvents.set(e.block, e);
    }
    const fx = layer(svg, 'fx');
    const exitLayer = layer(svg, 'exit');
    const cardsLayer = layer(svg, 'cards');
    const poolLayer = scene.pool ? layer(svg, 'pool') : null;

    // Where every card is right now, before anything is retargeted.
    const cardX = new Map<string, number>();
    for (const [id, r] of this.cards) cardX.set(id, r.node.v.X);

    // ---- Cards ----
    const targets: { id: string; x: number; w: number; h: number; meta: Omit<CardRec, 'node'> }[] =
      [];
    if (scene.next) {
      targets.push({
        id: NEXT_CARD_ID,
        x: scene.next.x,
        w: scene.next.w,
        h: scene.next.h,
        meta: {
          mined: false,
          number: scene.next.number,
          hash: NEXT_CARD_ID,
          parentHash: scene.snapshot.tip.hash,
        },
      });
    }
    for (const c of scene.cards) {
      targets.push({
        id: c.id,
        x: c.x,
        w: c.w,
        h: c.h,
        meta: {
          mined: true,
          number: c.block.number,
          hash: c.block.hash,
          parentHash: c.block.parentHash,
        },
      });
    }
    const targetIds = new Set(targets.map((t) => t.id));
    for (const t of targets) {
      const el = (
        t.id === NEXT_CARD_ID
          ? cardsLayer.querySelector('[data-next]')
          : cardsLayer.querySelector(`[data-card="${t.id}"]`)
      ) as SVGGElement | null;
      if (!el) throw new Error(`motion: card ${t.id} was laid out but is not in the DOM`);
      const v: Values = { X: t.x, Y: 0, w: t.w, h: t.h, op: 1, solid: 1, fresh: 0 };
      const rec = this.cards.get(t.id);
      if (!rec) {
        const created: CardRec = {
          node: this.animator.node(t.id, el, v, null, (n) => this.paintCard(n)),
          ...t.meta,
        };
        this.cards.set(t.id, created);
        if (boot) continue;
        const n = created.node;
        if (mined && mined.block === t.id) {
          // The landing block materializes: dashed until its commits land, then solid and fresh.
          n.v.solid = 0;
          n.paint(n);
          this.animator.go(
            n,
            { solid: 1 },
            {
              channel: 'solid',
              delay: Math.max(0, tr.contact - 80),
              dur: 420,
              ease: (u) => smoothstep(0, 1, u),
            }
          );
          this.animator.after(tr.contact, () => {
            n.v.fresh = 1;
            this.animator.go(n, { fresh: 0 }, { channel: 'fresh', dur: FRESH_MS, ease: freshEase });
          });
        } else if (t.id === NEXT_CARD_ID) {
          this.animator.go(n, { op: 1 }, { from: { op: 0 }, dur: 280, delay: 140 });
        } else {
          this.animator.go(n, { op: 1 }, { from: { op: 0 }, dur: 300 });
        }
        continue;
      }
      rec.node.el = el;
      Object.assign(rec, t.meta);
      const to = { X: t.x, w: t.w, h: t.h };
      if (this.animator.differs(rec.node, to, 0.01)) {
        this.animator.go(rec.node, to, { dur: settle ? 700 : 260 });
      }
    }
    const goneCards = new Set<string>();
    for (const [id, rec] of [...this.cards]) {
      if (targetIds.has(id)) continue;
      this.cards.delete(id);
      goneCards.add(id);
      const clone = departing.cards.get(id);
      if (!clone || boot) continue;
      strip(clone);
      exitLayer.appendChild(clone);
      rec.node.el = clone;
      rec.node.paint(rec.node);
      this.exitingCards.add(rec);
      const kind =
        cardEvents.get(id)?.kind === 'orphan' ? 'orphan' : id === NEXT_CARD_ID ? 'hide' : 'retire';
      this.animator.exit(rec.node, kind, 0, () => this.exitingCards.delete(rec));
    }

    // ---- Squares ----
    const containerX = (container: string, at: 'now' | 'before') => {
      if (container === 'pool') return 0;
      if (at === 'before') {
        const x = cardX.get(container);
        if (x === undefined) throw new Error(`motion: square container ${container} has no card`);
        return x;
      }
      const rec = this.cards.get(container);
      if (!rec) throw new Error(`motion: square container ${container} has no card`);
      return rec.node.v.X;
    };
    const els = new Map<string, SVGElement>();
    for (const root of [poolLayer, cardsLayer]) {
      root
        ?.querySelectorAll('[data-sq]')
        .forEach((e) => els.set(e.getAttribute('data-sq') as string, e as SVGElement));
    }
    const all: SceneSquare[] = [
      ...(scene.pool ? [...scene.pool.pending.squares, ...scene.pool.proposed.squares] : []),
      ...scene.cards.flatMap((c) => c.squares),
    ];
    const allIds = new Set(all.map((q) => q.id));
    const landingId = mined?.block ?? null;
    for (const q of all) {
      const el = els.get(q.id);
      if (!el) throw new Error(`motion: square ${q.id} was laid out but is not in the DOM`);
      const look = lookOf(q);
      const v: Values = { X: q.x, Y: q.y, s: q.s, op: 1 };
      const e = txEvents.get(q.id);
      let rec = this.squares.get(q.id);
      if (rec && rec.node.exiting) rec = undefined;
      if (!rec) {
        const node = this.animator.node(q.id, el, v, look, paintSquareNode);
        this.squares.set(q.id, { node, container: q.container });
        if (boot) continue;
        const cx = containerX(q.container, 'now');
        const centre = { X: cx + q.x + q.s / 2, Y: q.y + q.s / 2 };
        const newCard = q.container !== 'pool' && !cardX.has(q.container);
        if (q.kind === 'cellbase' && q.container === landingId) {
          node.v.s = 0;
          node.paint(node);
          this.animator.go(node, { s: q.s }, { dur: 520, delay: tr.contact, birth: true });
          this.animator.ripple(fx, centre.X, centre.Y, 12, tok('jade'), tr.contact);
        } else if (e?.kind === 'arrive') {
          this.animator.go(
            node,
            { Y: q.y, op: 1 },
            { from: { Y: q.y - 10, op: 0 }, dur: 380, ease: E.fall }
          );
          this.animator.ripple(fx, centre.X, centre.Y, 8, tok('aqua'), 320);
        } else if (newCard && q.container !== landingId) {
          // Arrives with its whole card.
        } else {
          node.v.s = 0;
          node.paint(node);
          this.animator.go(node, { s: q.s }, { dur: 260, delay: e && 'at' in e ? e.at : 0 });
        }
        continue;
      }
      const n = rec.node;
      if (n.el !== el) {
        n.el = el;
        n.painted = '';
      }
      // Re-express the current position in the new container's coordinates.
      n.v.X += containerX(rec.container, 'before') - containerX(q.container, 'now');
      rec.container = q.container;
      n.paint(n);
      if (e?.kind === 'commit') {
        this.animator.go(n, v, {
          look,
          delay: 40 + Math.min(e.rank * 16, 420),
          dur: 720,
          ease: E.lob,
          arc: true,
          colorLate: true,
        });
      } else if (e?.kind === 'propose') {
        this.animator.go(n, v, { look, delay: 120 + Math.min(e.rank * 12, 300), dur: 560 });
      } else if (e?.kind === 'return') {
        this.animator.go(n, v, {
          look,
          delay: e.at + Math.min(e.rank * 12, 300),
          dur: 560,
          colorLate: true,
        });
      } else if (this.animator.differs(n, v) || !sameLook(n.look, look)) {
        this.animator.go(n, v, { look, dur: settle ? 700 : 300 });
      }
    }
    for (const [id, rec] of [...this.squares]) {
      if (allIds.has(id) || rec.node.exiting) continue;
      this.squares.delete(id);
      if (goneCards.has(rec.container)) continue;
      const clone = departing.squares.get(id);
      if (!clone || boot) continue;
      strip(clone);
      exitLayer.appendChild(clone);
      rec.node.v.X += containerX(rec.container, 'before');
      rec.node.el = clone;
      rec.node.painted = '';
      rec.node.paint(rec.node);
      const e = txEvents.get(id);
      const delay = e && 'at' in e ? e.at : 0;
      this.animator.exit(rec.node, e?.kind === 'drop' ? 'drop' : 'hide', delay);
    }

    // ---- Proposal dots resolve when the commit lands, not before ----
    for (const e of events) {
      if (e.kind !== 'resolve' || boot) continue;
      const key = `${e.shortId}|${e.uncleIndex ?? ''}`;
      const el = cardsLayer.querySelector(`[data-card="${e.block}"] [data-chip="${key}"]`);
      if (!el) continue; // past the two rows of dots: drawn as +N
      paintChip(el, e.from);
      this.animator.after(e.at, () => {
        paintChip(el, e.to);
        const rec = this.cards.get(e.block);
        if (rec) {
          const x = rec.node.v.X + Number(el.getAttribute('cx'));
          const y = Number(el.getAttribute('cy'));
          this.animator.ripple(fx, x, y, 7, tok('gold'));
        }
      });
    }

    this.paintChain();
  }

  private paintCard(n: MotionNode): void {
    const { X, Y, op, w, h, solid, fresh } = n.v;
    n.el.setAttribute('transform', `translate(${X.toFixed(2)},${Y.toFixed(2)})`);
    n.el.setAttribute('opacity', op.toFixed(3));
    const box = n.el.querySelector('[data-box]');
    if (!box) throw new Error(`motion: card ${n.id} has no box`);
    box.setAttribute('width', Math.max(1, w - 1).toFixed(2));
    box.setAttribute('height', Math.max(1, h - 1).toFixed(2));
    if (n.id === NEXT_CARD_ID) return;
    const hl = this.view.highlighted(n.id);
    // A settled tile has no outline: the dashed NEXT outline fades out as the block solidifies.
    box.setAttribute(
      'stroke-opacity',
      String(hl ? 0.7 : Math.round(Math.min(1, 0.6 * (1 - solid) + 0.6 * fresh) * 1000) / 1000)
    );
    box.setAttribute(
      'fill-opacity',
      String(hl ? 1 : Math.round((0.35 + 0.65 * solid) * 1000) / 1000)
    );
    if (solid < 0.5) box.setAttribute('stroke-dasharray', '3 2');
    else box.removeAttribute('stroke-dasharray');
    const chips = n.el.querySelector('[data-chips]');
    if (chips) {
      if (solid >= 1) chips.removeAttribute('opacity');
      else chips.setAttribute('opacity', (0.2 + 0.8 * solid).toFixed(3));
    }
  }

  /** The chain line and the window ruler follow the cards while they move. */
  private paintChain(): void {
    const svg = this.svg;
    const scene = this.scene;
    if (!svg || !scene) return;
    const { consensus } = scene.snapshot;
    const H = this.view.anchor();
    const slots: ChainSlotState[] = [];
    let next: { x: number; w: number } | null = null;
    let nextSlot: RulerSlot | null = null;
    for (const r of [...this.cards.values(), ...this.exitingCards]) {
      if (r.node.v.op <= 0.02) continue;
      if (!r.mined) {
        next = { x: r.node.v.X, w: r.node.v.w };
        nextSlot = { number: r.number, x: r.node.v.X, w: r.node.v.w, mined: false };
        continue;
      }
      slots.push({
        number: r.number,
        x: r.node.v.X,
        w: r.node.v.w,
        mined: true,
        hash: r.hash,
        parentHash: r.parentHash,
        solid: r.node.v.solid,
      });
    }
    const paths = chainPaths(slots, next, H, consensus, scene.viewW);
    const ruler = windowRuler(
      nextSlot ? [nextSlot, ...slots] : slots,
      H,
      consensus,
      scene.viewW,
      windowLabel(H, scene.windowTip, consensus)
    );
    const set = (sel: string, d: string) => svg.querySelector(sel)?.setAttribute('d', d || 'M0 0');
    set('[data-path="solid"]', paths.solid);
    set('[data-path="pend"]', paths.pend);
    set('[data-path="win"]', paths.win);
    set('[data-path="tail"]', paths.tail);
    const tail = svg.querySelector('[data-path="tail"]') as SVGElement | null;
    if (tail) {
      tail.style.setProperty('stroke', tok(ruler.tailOpen ? 'gold' : 'jade'));
      tail.setAttribute('stroke-opacity', ruler.tailOpen ? '0.7' : '0.25');
    }
    set('[data-window-marks]', windowMarksPath(ruler));
    set('[data-ruler-ticks]', rulerPath(ruler));
    const label = svg.querySelector('[data-window-label]');
    if (label) {
      if (ruler.label === null) label.setAttribute('visibility', 'hidden');
      else {
        label.setAttribute('x', ruler.label.x.toFixed(1));
        label.removeAttribute('visibility');
      }
    }
  }
}
