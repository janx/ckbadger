import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  Animator,
  E,
  FRESH_MS,
  bezier,
  birthEase,
  browserClock,
  lookPaint,
  paintSquareNode,
  type Look,
  type MotionNode,
} from '@/lib/pipeline/motion';
import { STAGE_BASE, agedPaint, mixColor, paintSquare } from '@/lib/pipeline/encoding';
import { GUT } from '@/lib/pipeline/layout';
import { color } from '@/lib/palette';

const NS = 'http://www.w3.org/2000/svg';

function svgLayer() {
  const svg = document.createElementNS(NS, 'svg');
  const g = document.createElementNS(NS, 'g');
  svg.appendChild(g);
  document.body.appendChild(svg);
  return g;
}

function square(animator: Animator, layer: SVGGElement, X = 0, Y = 0): MotionNode {
  const el = document.createElementNS(NS, 'rect');
  layer.appendChild(el);
  return animator.node(
    'tx',
    el,
    { X, Y, s: 6, op: 1, flare: 0 },
    { base: STAGE_BASE.pending, style: 'solid', step: 1, cellbase: false, age: null },
    paintSquareNode
  );
}

describe('easing curves', () => {
  it('keeps the prototype bezier ends and shapes', () => {
    for (const f of [E.enter, E.fade, E.fall, E.lob]) {
      expect(f(0)).toBe(0);
      expect(f(1)).toBe(1);
    }
    expect(E.lob(0.5)).toBeCloseTo(0.5, 10);
    // enter is fast-out, fall is slow-in.
    expect(E.enter(0.3)).toBeGreaterThan(0.5);
    expect(E.fall(0.3)).toBeLessThan(0.2);
    const linear = bezier(0, 0, 1, 1);
    expect(linear(0.37)).toBeCloseTo(0.37, 4);
  });

  it('overshoots a birth by 8 % and settles at full size', () => {
    expect(birthEase(0)).toBe(0);
    expect(birthEase(0.78)).toBeCloseTo(1.08, 6);
    expect(birthEase(1)).toBeCloseTo(1, 6);
  });
});

describe('paintSquareNode', () => {
  const node = (
    s: number,
    cellbase: boolean,
    o: { age?: number | null; flare?: number; cut?: SVGElement | null } = {}
  ) => {
    const el = document.createElementNS(NS, cellbase ? 'path' : 'rect');
    const n: MotionNode = {
      id: 'q',
      el,
      cut: o.cut ?? null,
      v: { X: 10, Y: 20, s, op: 1, flare: o.flare ?? 0 },
      target: { X: 10, Y: 20, s, op: 1, flare: o.flare ?? 0 },
      look: {
        base: cellbase ? STAGE_BASE.cellbase : STAGE_BASE.committed,
        style: 'solid',
        step: 0,
        cellbase,
        age: o.age === undefined ? 0 : o.age,
      },
      paint: paintSquareNode,
      tws: new Map(),
      exiting: false,
      painted: '',
    };
    paintSquareNode(n);
    return n;
  };
  const paint = (s: number, cellbase: boolean) => node(s, cellbase).el;

  it('keeps a square that has no size yet (a cellbase before its birth) invisible', () => {
    expect(paint(0, true).getAttribute('opacity')).toBe('0.000');
    expect(paint(0.4, false).getAttribute('opacity')).toBe('0.000');
  });

  it('shows a square at its own opacity once it has a size', () => {
    expect(paint(0.5, true).getAttribute('opacity')).toBe('1.000');
    const rect = paint(9, false);
    expect(rect.getAttribute('opacity')).toBe('1.000');
    expect(rect.getAttribute('width')).toBe('9.00');
  });

  it('paints a tx square with square corners, as the static square is drawn (U-02, R1)', () => {
    for (const s of [0.5, 3, 9]) expect(paint(s, false).getAttribute('rx')).toBeNull();
  });

  // Lane R: a mark on an older block steps back by its fill, as the static render paints it.
  it('paints a square on a block `age` places back as the static square: mixed toward its cut', () => {
    for (const age of [0, 1, 7]) {
      const n = node(15, false, { age });
      const own = agedPaint(paintSquare('committed', 'solid', 0), 'square', age);
      expect(n.el.style.getPropertyValue('fill')).toBe(own.fill);
    }
    const cb = node(15, true, { age: 3 });
    expect(cb.el.style.getPropertyValue('fill')).toBe(
      agedPaint(paintSquare('cellbase', 'solid', 0), 'cellbase', 3).fill
    );
    // a pool square does not age
    const pool = node(15, false, { age: null });
    expect(pool.el.style.getPropertyValue('fill')).toBe(paintSquare('committed', 'solid', 0).fill);
  });

  it('lights a square the neon white at a full flare and gives it back its own fill at none', () => {
    const own = paintSquare('committed', 'solid', 0).fill;
    expect(node(15, false, { flare: 1 }).el.style.getPropertyValue('fill')).toBe(
      color('neon-white')
    );
    expect(node(15, false, { flare: 0 }).el.style.getPropertyValue('fill')).toBe(own);
    expect(node(15, false, { flare: 0.5 }).el.style.getPropertyValue('fill')).toBe(
      mixColor(own, color('neon-white'), 0.5)
    );
    const look: Look = {
      base: STAGE_BASE.cellbase,
      style: 'solid',
      step: 0,
      cellbase: true,
      age: 2,
    };
    expect(lookPaint(look, 1).fill).toBe(color('neon-white'));
    expect(lookPaint(look, 0).fill).toBe(
      agedPaint(paintSquare('cellbase', 'solid', 0), 'cellbase', 2).fill
    );
  });

  it('carries its cut with it: the square grown by the gutter, never seen without its square', () => {
    const cut = document.createElementNS(NS, 'rect');
    const n = node(15, false, { cut });
    expect(['x', 'y', 'width', 'height', 'opacity'].map((a) => cut.getAttribute(a))).toEqual([
      (10 - GUT).toFixed(2),
      (20 - GUT).toFixed(2),
      (15 + 2 * GUT).toFixed(2),
      (15 + 2 * GUT).toFixed(2),
      '1.000',
    ]);
    // moved and shrunk to nothing: the cut follows, and goes with the square
    n.v.X = 40;
    n.v.s = 0.3;
    paintSquareNode(n);
    expect(cut.getAttribute('x')).toBe((40 - GUT).toFixed(2));
    expect(cut.getAttribute('opacity')).toBe('0.000');
  });

  it('refuses a square node without a flare value', () => {
    const n = node(15, false);
    delete n.v.flare;
    n.painted = '';
    expect(() => paintSquareNode(n)).toThrow(/flare/);
  });
});

describe('the landing flare', () => {
  it('settles a landed block\u2019s marks from the neon white over 400 ms', () => {
    expect(FRESH_MS).toBe(400);
  });
});

describe('Animator under fake timers and rAF', () => {
  let layer: SVGGElement;

  beforeEach(() => {
    vi.useFakeTimers({
      toFake: [
        'setTimeout',
        'clearTimeout',
        'requestAnimationFrame',
        'cancelAnimationFrame',
        'performance',
      ],
    });
    layer = svgLayer();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it('eases a tween along its curve and lands exactly on the target', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    const t0 = performance.now();
    a.go(n, { X: 100 }, { dur: 260, ease: E.enter });

    vi.advanceTimersByTime(128);
    const u = (performance.now() - t0) / 260;
    const xMid = n.v.X;
    expect(xMid).toBeGreaterThan(0);
    expect(xMid).toBeLessThan(100);
    // Painted from the last frame, which ran at most one frame before now.
    expect(xMid).toBeLessThanOrEqual(100 * E.enter(u) + 1e-9);
    expect(xMid).toBeGreaterThanOrEqual(100 * E.enter(u - 17 / 260) - 1e-9);
    expect(Number(n.el.getAttribute('x'))).toBeCloseTo(xMid, 2);

    vi.advanceTimersByTime(200);
    expect(n.v.X).toBe(100);
    expect(n.el.getAttribute('x')).toBe('100.00');
    expect(a.isBusy()).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('waits for the delay before moving', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    a.go(n, { Y: 50 }, { dur: 100, delay: 200 });
    vi.advanceTimersByTime(150);
    expect(n.v.Y).toBe(0);
    vi.advanceTimersByTime(200);
    expect(n.v.Y).toBe(50);
  });

  it('lobs a commit over both ends of its path', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer, 0, 100);
    let lowest = Infinity;
    a.go(n, { X: 200, Y: 120 }, { dur: 720, ease: E.lob, arc: true });
    for (let i = 0; i < 50; i++) {
      vi.advanceTimersByTime(16);
      lowest = Math.min(lowest, n.v.Y);
    }
    // The control point sits 24 px + 10 % of the distance above the higher end.
    expect(lowest).toBeLessThan(90);
    expect(n.v).toMatchObject({ X: 200, Y: 120 });
  });

  it('lobs a commit down beside both ends in a column: straight down in height, bowed out to the right', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer, 140, 20);
    const ys: number[] = [];
    let widest = -Infinity;
    a.go(n, { X: 110, Y: 80 }, { dur: 720, ease: E.lob, arc: 'beside' });
    for (let i = 0; i < 50; i++) {
      vi.advanceTimersByTime(16);
      ys.push(n.v.Y);
      widest = Math.max(widest, n.v.X);
    }
    // The control point sits beside the wider end at mid height: the height only ever goes down.
    for (let i = 1; i < ys.length; i++) expect(ys[i]).toBeGreaterThanOrEqual(ys[i - 1]);
    expect(widest).toBeGreaterThan(140 + 8);
    expect(n.v).toMatchObject({ X: 110, Y: 80 });
  });

  it('retires a column row downwards and steps an orphan off the chain sideways', () => {
    const a = new Animator(browserClock, false);
    const retired = square(a, layer, 6, 290);
    const orphan = square(a, layer, 6, 246);
    a.exit(retired, 'retire', 0, undefined, 'column');
    a.exit(orphan, 'orphan', 0, undefined, 'column');
    vi.advanceTimersByTime(300);
    expect(retired.v.X).toBe(6);
    expect(retired.v.Y).toBeGreaterThan(290);
    expect(orphan.v.Y).toBe(246);
    expect(orphan.v.X).toBeGreaterThan(6);
    vi.advanceTimersByTime(400);
    expect(retired.el.isConnected).toBe(false);
    expect(orphan.el.isConnected).toBe(false);
  });

  it('switches the colour late for colour-late tweens', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    a.go(
      n,
      { X: 10 },
      {
        dur: 1000,
        colorLate: true,
        look: { base: STAGE_BASE.committed, style: 'dashed', step: 0, cellbase: false, age: null },
      }
    );
    vi.advanceTimersByTime(500);
    expect(n.look?.base).toBe(STAGE_BASE.pending);
    expect(n.look?.style).toBe('solid');
    vi.advanceTimersByTime(600);
    expect(n.look).toMatchObject({ base: STAGE_BASE.committed, style: 'dashed', step: 0 });
    expect(n.el.getAttribute('stroke-dasharray')).toBe('2 1.5');
  });

  it('removes a node once its exit has finished', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    a.exit(n, 'retire');
    vi.advanceTimersByTime(300);
    expect(n.el.isConnected).toBe(true);
    expect(n.v.X).toBeGreaterThan(0);
    vi.advanceTimersByTime(400);
    expect(n.el.isConnected).toBe(false);
    expect(a.isBusy()).toBe(false);
  });

  it('drops a node by shrinking it toward ember, then removes it', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    a.exit(n, 'drop');
    vi.advanceTimersByTime(60 + 700);
    expect(n.v.s).toBeLessThan(6);
    expect(n.look?.base).toContain('var(--pipeline-drop)');
    vi.advanceTimersByTime(300);
    expect(n.el.isConnected).toBe(false);
  });

  it('opens a ripple and removes it after 480 ms', () => {
    const a = new Animator(browserClock, false);
    a.ripple(layer, 10, 10, 8, 'var(--pipeline-aqua)');
    const ring = layer.querySelector('circle') as SVGCircleElement;
    expect(ring).not.toBeNull();
    vi.advanceTimersByTime(240);
    expect(Number(ring.getAttribute('r'))).toBeGreaterThan(0);
    vi.advanceTimersByTime(260);
    expect(ring.isConnected).toBe(false);
  });

  it('runs a scheduled callback at its time', () => {
    const a = new Animator(browserClock, false);
    const fn = vi.fn();
    a.after(500, fn);
    vi.advanceTimersByTime(480);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(40);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('shows the final state at once with reduced motion', () => {
    const a = new Animator(browserClock, true);
    const n = square(a, layer);
    const onEnd = vi.fn();
    a.go(n, { X: 100, op: 0.5 }, { dur: 500, delay: 100, onEnd });
    expect(n.v).toMatchObject({ X: 100, op: 0.5 });
    expect(onEnd).toHaveBeenCalledTimes(1);
    expect(onEnd.mock.calls[0][0]).toBe(n);
    expect(onEnd.mock.calls[0][1]).toBe(true);
    a.ripple(layer, 0, 0, 8, 'red');
    const fn = vi.fn();
    a.after(1000, fn);
    expect(fn).toHaveBeenCalledTimes(1);
    a.exit(n, 'drop');
    expect(n.el.isConnected).toBe(false);
    expect(layer.querySelector('circle')).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('jumps running work to its end when reduced motion is switched on', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    const gone = square(a, layer);
    const fn = vi.fn();
    a.go(n, { X: 80 }, { dur: 1000 });
    a.exit(gone, 'retire');
    a.after(5000, fn);
    vi.advanceTimersByTime(100);
    a.setReduced(true);
    expect(n.v.X).toBe(80);
    expect(gone.el.isConnected).toBe(false);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(a.isBusy()).toBe(false);
  });
});
