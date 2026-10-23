import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  Animator,
  E,
  bezier,
  birthEase,
  browserClock,
  paintSquareNode,
  type MotionNode,
} from '@/lib/pipeline/motion';
import { STAGE_BASE } from '@/lib/pipeline/encoding';

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
    { X, Y, s: 6, op: 1 },
    { base: STAGE_BASE.pending, style: 'solid', step: 1, cellbase: false },
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

  it('switches the colour late for colour-late tweens', () => {
    const a = new Animator(browserClock, false);
    const n = square(a, layer);
    a.go(
      n,
      { X: 10 },
      {
        dur: 1000,
        colorLate: true,
        look: { base: STAGE_BASE.committed, style: 'dashed', step: 0, cellbase: false },
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

  it('calls a reward arc home once 40 % of its 2.2 s life has drawn the line', () => {
    const a = new Animator(browserClock, false);
    const onArrive = vi.fn();
    a.rewardArc(
      layer,
      () => [
        { X: 0, Y: 120 },
        { X: 300, Y: 120 },
      ],
      100,
      onArrive
    );
    vi.advanceTimersByTime(100 + 0.4 * 2200 - 20);
    expect(onArrive).not.toHaveBeenCalled();
    vi.advanceTimersByTime(40);
    expect(onArrive).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(2200);
    expect(onArrive).toHaveBeenCalledTimes(1);
    expect(layer.querySelector('path')).toBeNull();
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
    const arrive = vi.fn();
    a.ripple(layer, 0, 0, 8, 'red');
    a.rewardArc(layer, () => null, 0, arrive);
    expect(arrive).toHaveBeenCalledTimes(1);
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
