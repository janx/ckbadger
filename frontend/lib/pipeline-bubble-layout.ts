interface BubbleLike {
  id: string;
  left: number;
  top: number;
  widthPx: number;
  heightPx: number;
}

const DEFAULT_BOUND_MIN = 0.08;
const DEFAULT_BOUND_MAX = 0.92;
const PLOT_WIDTH_PX = 116;
const PLOT_HEIGHT_PX = 74;

function clamp(value: number, min: number, max: number): number {
  if (value < min) return min;
  if (value > max) return max;
  return value;
}

function seededUnit(seed: string): number {
  let hash = 0;
  for (let i = 0; i < seed.length; i += 1) {
    hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  }
  return (hash % 1000) / 999;
}

function overlaps(a: BubbleLike, b: BubbleLike): boolean {
  const dx = Math.abs(a.left - b.left);
  const dy = Math.abs(a.top - b.top);
  const minDx = ((a.widthPx + b.widthPx) * 0.5) / PLOT_WIDTH_PX + 0.008;
  const minDy = ((a.heightPx + b.heightPx) * 0.5) / PLOT_HEIGHT_PX + 0.008;
  return dx < minDx && dy < minDy;
}

export function resolveBubbleOverlaps<T extends BubbleLike>(bubbles: T[]): T[] {
  const placed: T[] = [];

  bubbles.forEach((bubble) => {
    const baseLeft = clamp(bubble.left, DEFAULT_BOUND_MIN, DEFAULT_BOUND_MAX);
    const baseTop = clamp(bubble.top, DEFAULT_BOUND_MIN, DEFAULT_BOUND_MAX);

    let candidate = { ...bubble, left: baseLeft, top: baseTop };

    for (let i = 0; i < 24; i += 1) {
      if (!placed.some((existing) => overlaps(existing, candidate))) {
        break;
      }

      const ring = Math.floor(i / 6) + 1;
      const radius = 0.016 * ring;
      const angle = (seededUnit(`${bubble.id}:${i}`) * 2 + i * 0.31) * Math.PI;
      const nextLeft = baseLeft + Math.cos(angle) * radius;
      const nextTop = baseTop + Math.sin(angle) * radius;

      candidate = {
        ...candidate,
        left: clamp(nextLeft, DEFAULT_BOUND_MIN, DEFAULT_BOUND_MAX),
        top: clamp(nextTop, DEFAULT_BOUND_MIN, DEFAULT_BOUND_MAX),
      };
    }

    placed.push(candidate);
  });

  return placed;
}
