import { describe, expect, it } from 'vitest';
import { resolveBubbleOverlaps } from '@/lib/pipeline-bubble-layout';

describe('resolveBubbleOverlaps', () => {
  it('spreads bubbles that start at identical coordinates', () => {
    const source = [
      { id: 'a', left: 0.5, top: 0.5, widthPx: 18, heightPx: 12 },
      { id: 'b', left: 0.5, top: 0.5, widthPx: 18, heightPx: 12 },
      { id: 'c', left: 0.5, top: 0.5, widthPx: 18, heightPx: 12 },
    ];

    const positioned = resolveBubbleOverlaps(source);
    const uniquePoints = new Set(positioned.map((bubble) => `${bubble.left}:${bubble.top}`));

    expect(uniquePoints.size).toBeGreaterThan(1);
  });

  it('keeps all points inside plotting bounds', () => {
    const source = Array.from({ length: 8 }, (_, idx) => ({
      id: `edge-${idx}`,
      left: 0.08,
      top: 0.08,
      widthPx: 20,
      heightPx: 14,
    }));

    const positioned = resolveBubbleOverlaps(source);

    positioned.forEach((bubble) => {
      expect(bubble.left).toBeGreaterThanOrEqual(0.08);
      expect(bubble.left).toBeLessThanOrEqual(0.92);
      expect(bubble.top).toBeGreaterThanOrEqual(0.08);
      expect(bubble.top).toBeLessThanOrEqual(0.92);
    });
  });
});
