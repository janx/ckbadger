const MIN_SHIFT_DISTANCE_PX = 24;
const DEFAULT_SHIFT_DISTANCE_PX = 168;

interface LeftRect {
  left: number;
}

function collectAdjacentGaps(rects: LeftRect[]): number[] {
  const gaps: number[] = [];

  for (let i = 1; i < rects.length; i += 1) {
    const gap = Math.abs(rects[i].left - rects[i - 1].left);
    if (gap > 1) {
      gaps.push(gap);
    }
  }

  return gaps;
}

export function computeUniformShiftDistance(rects: LeftRect[]): number {
  const gaps = collectAdjacentGaps(rects);
  if (gaps.length === 0) return DEFAULT_SHIFT_DISTANCE_PX;

  const smallestGap = Math.min(...gaps);
  return Math.max(MIN_SHIFT_DISTANCE_PX, smallestGap);
}

export function computeUniformShiftDeltaX(rects: LeftRect[]): number {
  const distance = computeUniformShiftDistance(rects);
  const direction =
    rects.length >= 2 && rects[1].left < rects[0].left
      ? 1 // RTL layouts move visually right via positive translateX
      : -1; // LTR layouts move visually right via negative translateX
  return direction * distance;
}
