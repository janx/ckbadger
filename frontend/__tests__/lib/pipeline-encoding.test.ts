import { describe, expect, it } from 'vitest';
import {
  CHIP_PAINT,
  STEP_I,
  chipPaint,
  cyclesGlow,
  paintSquare,
  shadeStep,
  squareSide,
  txLook,
} from '@/lib/pipeline/encoding';

describe('squareSide: area = bytes', () => {
  it('maps the smallest tx to one cell and ≥ 8.4 KB to the 4-cell cap', () => {
    expect(squareSide(330)).toBe(1);
    expect(squareSide(8400)).toBe(4);
    expect(squareSide(98_000)).toBe(4);
  });

  it('rounds sqrt(size / 600) at the half-cell boundaries', () => {
    expect(squareSide(1349)).toBe(1);
    expect(squareSide(1350)).toBe(2);
    expect(squareSide(3749)).toBe(2);
    expect(squareSide(3750)).toBe(3);
    expect(squareSide(7349)).toBe(3);
    expect(squareSide(7350)).toBe(4);
  });

  it('rejects a non-positive size instead of drawing a square for it', () => {
    expect(() => squareSide(0)).toThrow(/size/);
    expect(() => squareSide(Number.NaN)).toThrow(/size/);
  });
});

describe('cycles → four lightness steps', () => {
  it('places the step thresholds on the log scale of the prototype', () => {
    // glow = (log10(cycles) − 6) / 2.2, steps at 0.15 / 0.4 / 0.7.
    expect(shadeStep(1_000_000)).toBe(0);
    expect(shadeStep(2_130_000)).toBe(0);
    expect(shadeStep(2_140_000)).toBe(1);
    expect(shadeStep(7_580_000)).toBe(1);
    expect(shadeStep(7_590_000)).toBe(2);
    expect(shadeStep(34_600_000)).toBe(2);
    expect(shadeStep(34_700_000)).toBe(3);
    expect(shadeStep(3_000_000_000)).toBe(3);
  });

  it('clamps the glow to [0, 1]', () => {
    expect(cyclesGlow(1)).toBe(0);
    expect(cyclesGlow(1e12)).toBe(1);
  });

  it('keeps the lightness steps of the prototype', () => {
    expect(STEP_I).toEqual([0.42, 0.62, 0.82, 1.0]);
  });
});

describe('txLook: one encoding per concept', () => {
  it('draws a committed tx with unknown cycles as a dashed square', () => {
    expect(txLook({ stage: 'committed', cycles: null, tooEarly: false })).toEqual({
      stage: 'committed',
      style: 'dashed',
      step: 0,
    });
    const paint = paintSquare('committed', 'dashed', 0);
    expect(paint.strokeDasharray).toBe('2 1.5');
    expect(paint.fillOpacity).toBe(0.06);
  });

  it('draws a proposed tx that is too early to commit as hollow', () => {
    expect(txLook({ stage: 'proposed', cycles: 3_000_000, tooEarly: true })).toEqual({
      stage: 'proposed',
      style: 'hollow',
      step: 1,
    });
    const paint = paintSquare('proposed', 'hollow', 1);
    expect(paint.fillOpacity).toBe(0.14);
    expect(paint.strokeDasharray).toBeNull();
    expect(paint.stroke).toContain('var(--pipeline-proposed)');
  });

  it('fills solid squares by mixing the stage hue over the surface at the step lightness', () => {
    const paint = paintSquare('pending', 'solid', 2);
    expect(paint.fill).toBe(
      'color-mix(in srgb, var(--pipeline-pending) 82%, var(--pipeline-surface))'
    );
    expect(paint.stroke).toBeNull();
    expect(paint.fillOpacity).toBeNull();
  });

  it('never uses cycles shade for the cellbase', () => {
    expect(txLook({ stage: 'cellbase', cycles: 9e9, tooEarly: false })).toEqual({
      stage: 'cellbase',
      style: 'solid',
      step: 0,
    });
  });

  it('only allows "too early" for proposed txs', () => {
    expect(() => txLook({ stage: 'pending', cycles: 1, tooEarly: true })).toThrow(/too early/);
  });
});

describe('proposal chip states', () => {
  it('draws early as hollow gold, waiting as solid gold, committed as dim jade, expired grey', () => {
    expect(chipPaint('early')).toEqual({
      fill: 'var(--pipeline-proposed)',
      fillOpacity: 0.12,
      stroke: 'var(--pipeline-gold)',
    });
    expect(chipPaint('waiting').fillOpacity).toBe(1);
    expect(chipPaint('waiting').stroke).toBeNull();
    expect(chipPaint('waiting').fill).toContain('var(--pipeline-proposed)');
    expect(chipPaint('committed').fill).toContain('var(--pipeline-committed)');
    expect(chipPaint('expired').fill).toContain('var(--pipeline-ink-dim)');
    expect(Object.keys(CHIP_PAINT).sort()).toEqual(['committed', 'early', 'expired', 'waiting']);
  });
});
