import type { PipelineProposalState } from '@/lib/api';
import { color } from '@/lib/palette';
import {
  GHOST,
  PLATE_EDGE,
  PLATE_FILL,
  STAGE_BASE,
  agedChip,
  agedPaint,
  diamondPath,
  mixColor,
  paintFromBase,
  tok,
  type ShadeStep,
  type SquarePaint,
  type SquareStyle,
} from '@/lib/pipeline/encoding';
import {
  GUT,
  axisOf,
  cardBox,
  windowLabel,
  type BlockSpan,
  type BoxRect,
  type ChainSlotState,
  type Orientation,
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
  /** Blocks behind the tip of the block it sits on (its fill steps back); null in a pool well. */
  age: number | null;
}

interface Tween {
  t0: number;
  dur: number;
  from: Values;
  to: Values;
  keys: string[];
  ease: Ease;
  arc: false | 'over' | 'beside';
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
  /**
   * A square's cut on its plate (null in a pool well, and for a card): it moves, grows and fades
   * with the square, so a cut never stands empty and a square never stands on bare plate.
   */
  cut: SVGElement | null;
  /** Current values: X, Y, s, op, flare for squares; X, Y, w, h, op, solid, plate for cards. */
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
  /**
   * Lob along a quadratic arc (a commit flying into its block): `true` rises above both ends, as a
   * commit flies right into a row; `'beside'` bows out to the right of both ends while it goes
   * down, as a commit falls into a column's NEXT row (its height moves straight down).
   */
  arc?: boolean | 'beside';
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

/** What a landed block's marks light to at contact: the phosphor white of a live figure. */
export const FLARE_INK = color('neon-white');

/**
 * The paint of a square node: its look's own paint (the cellbase its base), aged on the block it
 * sits on (`agedPaint`, as the static square is painted), then lit toward the flare ink by
 * `flare` (1 = the neon white, 0 = its own fill).
 */
export function lookPaint(look: Look, flare: number): SquarePaint {
  const own: SquarePaint = look.cellbase
    ? { fill: look.base, fillOpacity: null, stroke: null, strokeDasharray: null }
    : paintFromBase(look.base, look.style, look.step);
  const aged = agedPaint(own, look.cellbase ? 'cellbase' : 'square', look.age);
  return {
    ...aged,
    fill: mixColor(aged.fill, FLARE_INK, flare),
    stroke: aged.stroke === null ? null : mixColor(aged.stroke, FLARE_INK, flare),
  };
}

const setRect = (el: Element, r: BoxRect) => {
  el.setAttribute('x', String(r.x));
  el.setAttribute('y', String(r.y));
  el.setAttribute('width', String(r.width));
  el.setAttribute('height', String(r.height));
};

/** Paints a square node (rect, or the cellbase diamond) and its cut from its values and look. */
export function paintSquareNode(n: MotionNode): void {
  const { X, Y, op, flare } = n.v;
  if (flare === undefined) throw new Error(`motion: square ${n.id} has no flare value`);
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
  }
  // A square born from nothing stays unseen until it has a size (a 0-size diamond is still a dot).
  const shown = (n.v.s < 0.5 ? 0 : op).toFixed(3);
  el.setAttribute('opacity', shown);
  // Its cut: the square's cell grown by the gutter, seen exactly while the square is.
  if (n.cut) {
    n.cut.setAttribute('x', (X + off - GUT).toFixed(2));
    n.cut.setAttribute('y', (Y + off - GUT).toFixed(2));
    n.cut.setAttribute('width', (s + 2 * GUT).toFixed(2));
    n.cut.setAttribute('height', (s + 2 * GUT).toFixed(2));
    n.cut.setAttribute('opacity', shown);
  }
  const key = `${look.base}|${look.style}|${look.step}|${look.age}|${flare}`;
  if (key === n.painted) return;
  n.painted = key;
  const p = lookPaint(look, flare);
  el.style.setProperty('fill', p.fill);
  if (look.cellbase) return;
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
      cut: null,
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
      arc: o.arc === 'beside' ? 'beside' : o.arc ? 'over' : false,
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

  /**
   * Animate a node out and remove its element; `onGone` runs once it is removed. A retiring card
   * moves on along the chain's direction (`along`): right in a row, down in a column; an orphan
   * steps off the chain across it.
   */
  exit(
    n: MotionNode,
    kind: ExitKind,
    delay = 0,
    onGone?: () => void,
    along: Orientation = 'row'
  ): void {
    n.exiting = true;
    n.tws.clear();
    const done = () => {
      n.el.remove();
      n.cut?.remove();
      n.tws.clear();
      this.active.delete(n);
      onGone?.();
    };
    if (this.reduced) {
      done();
      return;
    }
    const row = along === 'row';
    if (kind === 'hide') this.go(n, { op: 0 }, { dur: 200, delay, ease: E.fade, onEnd: done });
    else if (kind === 'retire') {
      const to: Values = row ? { X: n.v.X + 18, op: 0 } : { Y: n.v.Y + 18, op: 0 };
      this.go(n, to, { dur: 600, delay, ease: E.fade, onEnd: done });
    } else if (kind === 'orphan') {
      const to: Values = row ? { Y: n.v.Y + 12, op: 0 } : { X: n.v.X + 12, op: 0 };
      this.go(n, to, { dur: 600, delay, ease: E.fade, onEnd: done });
    } else {
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
      const over = tw.arc === 'over';
      const cx = over ? (f.X + t.X) / 2 : Math.max(f.X, t.X) + 24 + 0.1 * d;
      const cy = over ? Math.min(f.Y, t.Y) - 24 - 0.1 * d : (f.Y + t.Y) / 2;
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
  /** The cut of a departing square on a plate that stays, by the square's id: it leaves with it. */
  cuts: Map<string, SVGElement>;
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

const STRIP = [
  'data-card',
  'data-next',
  'data-sq',
  'data-cut',
  'data-chip',
  'data-number',
  'data-slot',
];

function strip(el: Element): void {
  for (const a of STRIP) {
    el.removeAttribute(a);
    el.querySelectorAll(`[${a}]`).forEach((x) => x.removeAttribute(a));
  }
}

/** A scene square's look; `age` the blocks behind the tip of its plate, null in a pool well. */
const lookOf = (q: SceneSquare, age: number | null): Look => ({
  base: STAGE_BASE[q.stage],
  style: q.style,
  step: q.step,
  cellbase: q.kind === 'cellbase',
  age,
});

const sameLook = (a: Look | null, b: Look) =>
  !!a && a.base === b.base && a.style === b.style && a.step === b.step && a.age === b.age;

/**
 * The landing flare: at contact a landed block's marks are the neon white and settle back to their
 * own fill by exponential decay (a 100 ms time constant), landing exactly on it after 400 ms.
 */
export const FRESH_MS = 400;
const freshEase = (u: number) => (1 - Math.exp(-4 * u)) / (1 - Math.exp(-4));

/** Paints a proposal dot in `state` on a block `age` places behind the tip. */
function paintChip(el: Element, state: PipelineProposalState, age: number): void {
  const p = agedChip(state, age);
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
  /** The orientation of the scene the records were made for (a card's box is laid out by it). */
  private orientation: Orientation = 'row';
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

  /**
   * A change of orientation redraws the canvas in place: nothing flies between a row and a column.
   * Every record is dropped, so the next sync boots.
   */
  private reset(): void {
    this.animator.finishAll();
    for (const c of this.exitingCards) c.node.el.remove();
    this.exitingCards.clear();
    this.cards.clear();
    this.squares.clear();
  }

  /** Called before React commits `next`: clone what is about to disappear. */
  captureDeparting(svg: SVGSVGElement, prev: Scene, next: Scene): Departing {
    const out: Departing = { cards: new Map(), squares: new Map(), cuts: new Map() };
    if (prev.orientation !== next.orientation) return out;
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
      const cut = cardsLayer.querySelector(`[data-cut="${q.id}"]`);
      if (cut) out.cuts.set(q.id, cut.cloneNode(true) as SVGElement);
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
    const turned = !!prev && prev.orientation !== scene.orientation;
    if (turned) this.reset();
    this.orientation = scene.orientation;
    const boot = !prev || turned || (fresh && tr.mode === 'boot');
    const along = scene.orientation;
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
    const cardAt = new Map<string, { X: number; Y: number }>();
    for (const [id, r] of this.cards) cardAt.set(id, { X: r.node.v.X, Y: r.node.v.Y });

    // ---- Cards ----
    const targets: {
      id: string;
      x: number;
      y: number;
      w: number;
      h: number;
      meta: Omit<CardRec, 'node'>;
    }[] = [];
    if (scene.next) {
      targets.push({
        id: NEXT_CARD_ID,
        x: scene.next.x,
        y: scene.next.y,
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
        y: c.y,
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
      const v: Values = { X: t.x, Y: t.y, w: t.w, h: t.h, op: 1, solid: 1, plate: 1 };
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
          // The landing block is NEXT's dashed outline until its commits land; at contact its
          // plate is set with its cuts and its stone, and its marks flare (below). Its dots enter
          // around contact.
          n.v.solid = 0;
          n.v.plate = 0;
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
            n.v.plate = 1;
            n.paint(n);
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
      // A row's cards move right, a column's rows move down: both are a move to the new origin.
      const to = { X: t.x, Y: t.y, w: t.w, h: t.h };
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
      this.animator.exit(rec.node, kind, 0, () => this.exitingCards.delete(rec), along);
    }

    // ---- Squares ----
    const ORIGIN = { X: 0, Y: 0 };
    const containerAt = (container: string, at: 'now' | 'before') => {
      if (container === 'pool') return ORIGIN;
      if (at === 'before') {
        const p = cardAt.get(container);
        if (!p) throw new Error(`motion: square container ${container} has no card`);
        return p;
      }
      const rec = this.cards.get(container);
      if (!rec) throw new Error(`motion: square container ${container} has no card`);
      return { X: rec.node.v.X, Y: rec.node.v.Y };
    };
    const els = new Map<string, SVGElement>();
    for (const root of [poolLayer, cardsLayer]) {
      root
        ?.querySelectorAll('[data-sq]')
        .forEach((e) => els.set(e.getAttribute('data-sq') as string, e as SVGElement));
    }
    // The cuts a plate draws under its squares, by square: a square in a pool well has none.
    const cutEls = new Map<string, SVGElement>();
    cardsLayer
      .querySelectorAll('[data-cut]')
      .forEach((e) => cutEls.set(e.getAttribute('data-cut') as string, e as SVGElement));
    // A plate's marks step back by their fill, by the blocks it stands behind the tip.
    const ageOf = new Map(scene.cards.map((c, i) => [c.id, i]));
    const ageIn = (container: string): number | null => {
      if (container === 'pool') return null;
      const age = ageOf.get(container);
      if (age === undefined) throw new Error(`motion: square container ${container} has no card`);
      return age;
    };
    const all: SceneSquare[] = [
      ...(scene.pool ? [...scene.pool.pending.squares, ...scene.pool.proposed.squares] : []),
      ...scene.cards.flatMap((c) => c.squares),
    ];
    const allIds = new Set(all.map((q) => q.id));
    const landingId = mined?.block ?? null;
    for (const q of all) {
      const el = els.get(q.id);
      if (!el) throw new Error(`motion: square ${q.id} was laid out but is not in the DOM`);
      const look = lookOf(q, ageIn(q.container));
      const v: Values = { X: q.x, Y: q.y, s: q.s, op: 1 };
      const e = txEvents.get(q.id);
      let rec = this.squares.get(q.id);
      if (rec && rec.node.exiting) rec = undefined;
      if (!rec) {
        const node = this.animator.node(q.id, el, { ...v, flare: 0 }, look, paintSquareNode);
        node.cut = cutEls.get(q.id) ?? null;
        node.paint(node);
        this.squares.set(q.id, { node, container: q.container });
        if (boot) continue;
        const at = containerAt(q.container, 'now');
        const centre = { X: at.X + q.x + q.s / 2, Y: at.Y + q.y + q.s / 2 };
        const newCard = q.container !== 'pool' && !cardAt.has(q.container);
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
      // its cut where it is now: none in a well, its plate's when it moved onto one
      n.cut = cutEls.get(q.id) ?? null;
      // Re-express the current position in the new container's coordinates.
      const was = containerAt(rec.container, 'before');
      const now = containerAt(q.container, 'now');
      n.v.X += was.X - now.X;
      n.v.Y += was.Y - now.Y;
      rec.container = q.container;
      n.paint(n);
      if (e?.kind === 'commit') {
        this.animator.go(n, v, {
          look,
          delay: 40 + Math.min(e.rank * 16, 420),
          dur: 720,
          ease: E.lob,
          // Into NEXT: over the gap to the right in a row, down from the proposed row in a column.
          arc: along === 'row' ? true : 'beside',
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
      // A square leaving a plate that stays takes its cut with it, under it.
      const cut = departing.cuts.get(id) ?? null;
      if (cut) {
        strip(cut);
        exitLayer.appendChild(cut);
      }
      strip(clone);
      exitLayer.appendChild(clone);
      const was = containerAt(rec.container, 'before');
      rec.node.v.X += was.X;
      rec.node.v.Y += was.Y;
      rec.node.el = clone;
      rec.node.cut = cut;
      rec.node.painted = '';
      rec.node.paint(rec.node);
      const e = txEvents.get(id);
      const delay = e && 'at' in e ? e.at : 0;
      this.animator.exit(rec.node, e?.kind === 'drop' ? 'drop' : 'hide', delay);
    }

    // ---- The landing flare: at contact the landed block's marks light, then settle ----
    if (mined && !boot && !this.animator.isReduced) {
      const block = mined.block;
      this.animator.after(tr.contact, () => {
        for (const r of this.squares.values()) {
          if (r.container !== block || r.node.exiting) continue;
          this.animator.go(
            r.node,
            { flare: 0 },
            { from: { flare: 1 }, channel: 'flare', dur: FRESH_MS, ease: freshEase }
          );
        }
      });
    }

    // ---- Proposal dots resolve when the commit lands, not before ----
    for (const e of events) {
      if (e.kind !== 'resolve' || boot) continue;
      const key = `${e.shortId}|${e.uncleIndex ?? ''}`;
      const el = cardsLayer.querySelector(`[data-card="${e.block}"] [data-chip="${key}"]`);
      if (!el) continue; // past the two rows of dots: drawn as +N
      const age = ageOf.get(e.block);
      if (age === undefined) throw new Error(`motion: dot ${key} of ${e.block} has no card`);
      paintChip(el, e.from, age);
      this.animator.after(e.at, () => {
        paintChip(el, e.to, age);
        const rec = this.cards.get(e.block);
        if (rec) {
          const x = rec.node.v.X + Number(el.getAttribute('cx'));
          const y = rec.node.v.Y + Number(el.getAttribute('cy'));
          this.animator.ripple(fx, x, y, 7, tok('gold'));
        }
      });
    }

    this.paintChain();
  }

  /**
   * Repaints what the motion layer owns after a React commit that brought no new scene (a hover, a
   * pin): every card from its values (a landing block's box, its stone, cuts and strap, a plate's
   * edge) and the chain, so nothing React has just drawn shows a state the motion layer has not
   * reached (a gilt strap on a block that has no plate yet).
   */
  repaint(): void {
    for (const r of this.cards.values()) r.node.paint(r.node);
    this.paintChain();
  }

  /**
   * Paints a card from its values: its place and entrance opacity, and its box. NEXT's box is its
   * dashed outline (React paints it). A block's box is its plate once `plate` is set, its own tone
   * with no stroke at rest (a 1 px dim edge while it is highlighted), and until then NEXT's dashed
   * outline with neither its stone, its cuts, its gilt strap nor the edge drawn over the gilt.
   */
  private paintCard(n: MotionNode): void {
    const { X, Y, op, w, h, solid, plate } = n.v;
    n.el.setAttribute('transform', `translate(${X.toFixed(2)},${Y.toFixed(2)})`);
    n.el.setAttribute('opacity', op.toFixed(3));
    const box = n.el.querySelector('[data-box]') as SVGElement | null;
    if (!box) throw new Error(`motion: card ${n.id} has no box`);
    if (n.id === NEXT_CARD_ID) {
      setRect(box, cardBox(this.orientation, w, h, true));
      return;
    }
    const set = plate >= 1;
    const hl = set && this.view.highlighted(n.id);
    setRect(box, cardBox(this.orientation, w, h, !set || hl));
    box.style.setProperty('fill', set ? PLATE_FILL : 'none');
    box.style.setProperty('stroke', set ? (hl ? PLATE_EDGE : 'none') : GHOST.outline.stroke);
    if (set) {
      box.removeAttribute('stroke-opacity');
      box.removeAttribute('stroke-dasharray');
    } else {
      box.setAttribute('stroke-opacity', String(GHOST.outline.strokeOpacity));
      box.setAttribute('stroke-dasharray', GHOST.outline.dash);
    }
    // The stone, the cuts, the gilt strap and the edge drawn over it are the plate's: none while
    // the block is an outline.
    for (const el of n.el.querySelectorAll(
      '[data-stone], [data-cuts], [data-strap], [data-edge]'
    )) {
      if (set) el.removeAttribute('opacity');
      else el.setAttribute('opacity', '0');
    }
    // The dots enter as the block solidifies, and their "+N" (in the card's words group) with
    // them: an entrance, which ends at full strength (R4: no word is faded at rest).
    for (const el of n.el.querySelectorAll('[data-chips], [data-chips-label]')) {
      if (solid >= 1) el.removeAttribute('opacity');
      else el.setAttribute('opacity', (0.2 + 0.8 * solid).toFixed(3));
    }
  }

  /**
   * The strap and the window's words follow the cards while they move, drawn every frame from the
   * same library functions as the static render (`axis.chainPaths`, `axis.lane`): a landing block's
   * link is the dashed pair until its plate is set, and the tail is gilt while the window runs on.
   */
  private paintChain(): void {
    const svg = this.svg;
    const scene = this.scene;
    if (!svg || !scene) return;
    const { consensus } = scene.snapshot;
    const axis = axisOf(scene);
    const H = this.view.anchor();
    const slots: ChainSlotState[] = [];
    let next: { x: number; w: number } | null = null;
    let nextSlot: BlockSpan | null = null;
    for (const r of [...this.cards.values(), ...this.exitingCards]) {
      if (r.node.v.op <= 0.02) continue;
      const { X, Y, w, h } = r.node.v;
      const at = axis.along({ x: X, y: Y, w, h });
      if (!r.mined) {
        next = at;
        nextSlot = { number: r.number, ...at, mined: false };
        continue;
      }
      slots.push({
        number: r.number,
        ...at,
        mined: true,
        hash: r.hash,
        parentHash: r.parentHash,
        plate: r.node.v.plate >= 1,
      });
    }
    const paths = axis.chainPaths(slots, next, H, consensus);
    const lane = axis.lane(
      nextSlot ? [nextSlot, ...slots] : slots,
      H,
      consensus,
      windowLabel(H, scene.windowTip, consensus)
    );
    const set = (sel: string, d: string) => svg.querySelector(sel)?.setAttribute('d', d || 'M0 0');
    set('[data-path="solid"]', paths.solid);
    set('[data-path="pend"]', paths.pend);
    set('[data-path="win"]', paths.win);
    set('[data-path="tail"]', paths.tail);
    set('[data-path="tail-win"]', paths.tailWin);
    const label = svg.querySelector('[data-window-label]');
    if (label) {
      if (lane.label === null) label.setAttribute('visibility', 'hidden');
      else {
        label.setAttribute('x', lane.label.x0.toFixed(1));
        label.removeAttribute('visibility');
      }
    }
  }
}
