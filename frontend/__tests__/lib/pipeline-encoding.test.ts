import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import * as encoding from '@/lib/pipeline/encoding';
import {
  CHIP_PAINT,
  CUT_FILL,
  GHOST,
  PLATE_FILL,
  STEP_W,
  WELL_FILL,
  ageOpacity,
  agedFill,
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

  it('mixes this much white into the stage hue at each step: more cycles, lighter', () => {
    expect(STEP_W).toEqual([0, 0.18, 0.36, 0.54]);
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

  it('fills a solid square with the stage colour, mixed toward white by its cycles step', () => {
    // Step 0 is the stage colour itself.
    expect(paintSquare('pending', 'solid', 0).fill).toBe('var(--pipeline-pending)');
    const paint = paintSquare('pending', 'solid', 2);
    expect(paint.fill).toBe('color-mix(in srgb, white 36%, var(--pipeline-pending))');
    expect(paint.stroke).toBeNull();
    expect(paint.fillOpacity).toBeNull();
    expect(paintSquare('committed', 'solid', 3).fill).toBe(
      'color-mix(in srgb, white 54%, var(--pipeline-committed))'
    );
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

describe('proposal dot states', () => {
  it('draws early as hollow gold, and waiting, committed and expired in the dot tokens of a plate', () => {
    expect(chipPaint('early')).toEqual({
      fill: 'var(--pipeline-proposed)',
      fillOpacity: 0.12,
      stroke: 'var(--pipeline-gold)',
    });
    // Each solid dot is its own token, derived against the plate (lib/palette.ts), not a mix toward
    // white or the surface (by day those fell to 2.9 and 2.6 : 1 on the plate).
    expect(chipPaint('waiting')).toEqual({
      fill: 'var(--pipeline-dot-waiting)',
      fillOpacity: 1,
      stroke: null,
    });
    expect(chipPaint('committed')).toEqual({
      fill: 'var(--pipeline-dot-committed)',
      fillOpacity: 1,
      stroke: null,
    });
    expect(chipPaint('expired')).toEqual({
      fill: 'var(--pipeline-dot-expired)',
      fillOpacity: 1,
      stroke: null,
    });
    expect(Object.keys(CHIP_PAINT).sort()).toEqual(['committed', 'early', 'expired', 'waiting']);
  });
});

describe('materials: well, plate, cut and the predicted NEXT', () => {
  it('sinks a pool well below the surface and stands a block as a solid plate, cut where a mark is', () => {
    // the well is the theme's own token (22 % black on the surface at night, a grey tint by day)
    expect(WELL_FILL).toBe('var(--pipeline-well)');
    // a block is a plate of its own tone; every mark on it is set in the cut tone
    expect(PLATE_FILL).toBe('var(--pipeline-plate)');
    expect(CUT_FILL).toBe('var(--pipeline-cut)');
    // the tile (elevated with a trace of jade) is gone
    expect('TILE_FILL' in encoding).toBe(false);
  });

  it('marks predicted txs with dashed jade outlines and predicted proposals in gold', () => {
    expect(GHOST.square.stroke).toBe('var(--pipeline-jade)');
    expect(GHOST.square.dash).toBe('2 1.5');
    expect(GHOST.chip.stroke).toBe('var(--pipeline-gold)');
  });
});

describe('ageOpacity: older blocks step back', () => {
  it('is 1 at the tip and falls evenly to 0.4 at tip − 7', () => {
    expect(ageOpacity(0)).toBe(1);
    expect(ageOpacity(7)).toBeCloseTo(0.4, 10);
    const steps = [0, 1, 2, 3, 4, 5, 6, 7].map(ageOpacity);
    for (let i = 1; i < steps.length; i++) {
      expect(steps[i - 1] - steps[i]).toBeCloseTo(0.6 / 7, 10);
    }
  });

  it('stays at 0.4 beyond tip − 7', () => {
    expect(ageOpacity(8)).toBeCloseTo(0.4, 10);
    expect(ageOpacity(23)).toBeCloseTo(0.4, 10);
  });

  it('rejects an age that is not a count of blocks', () => {
    expect(() => ageOpacity(-1)).toThrow(/age/);
    expect(() => ageOpacity(1.5)).toThrow(/age/);
  });
});

// Direction F: a plate never fades, and a mark on it fades by mixing, not by opacity (opacity would
// show the plate through a square and the cut round it).
describe('agedFill: a mark steps back by mixing toward its ground', () => {
  const fill = 'var(--pipeline-committed)';

  it('keeps the mark its own colour at the tip', () => {
    expect(agedFill('square', fill, 0)).toBe(fill);
    expect(agedFill('dot', fill, 0)).toBe(fill);
  });

  it('mixes a square and the cellbase toward the cut, the mark keeping ageOpacity of itself', () => {
    expect(agedFill('square', fill, 7)).toBe(
      'color-mix(in srgb, var(--pipeline-committed) 40%, var(--pipeline-cut))'
    );
    expect(agedFill('cellbase', fill, 7)).toBe(
      'color-mix(in srgb, var(--pipeline-committed) 40%, var(--pipeline-cut))'
    );
    // one step back: 1 − 0.6 / 7 of itself
    expect(agedFill('square', fill, 1)).toBe(
      `color-mix(in srgb, ${fill} ${Math.round(ageOpacity(1) * 1000) / 10}%, var(--pipeline-cut))`
    );
    expect(agedFill('square', fill, 1)).toBe(
      'color-mix(in srgb, var(--pipeline-committed) 91.4%, var(--pipeline-cut))'
    );
  });

  it('mixes a dot toward the plate it stands on', () => {
    expect(agedFill('dot', 'var(--pipeline-dot-waiting)', 23)).toBe(
      'color-mix(in srgb, var(--pipeline-dot-waiting) 40%, var(--pipeline-plate))'
    );
  });

  it('rejects an age that is not a count of blocks and a mark it has no ground for', () => {
    expect(() => agedFill('square', fill, -1)).toThrow(/age/);
    expect(() => agedFill('plate' as never, fill, 1)).toThrow(/no ground/);
  });
});

// Rubbing × Song print, phase 2: phase 1 mapped amber to moss green one-to-one, which turned the
// ember a dropped transaction greys toward into a green. The ember is gold now, the warning hue.
describe('the drop ember', () => {
  const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
  it('greys a dropped tx toward the gold ember, not toward moss', () => {
    expect(css).toMatch(/--pipeline-ember: var\(--c-gold\);/);
    expect(css).not.toMatch(/--pipeline-ember: var\(--c-amber\)/);
    expect(css).toMatch(
      /--pipeline-drop: color-mix\(in srgb, var\(--c-pipeline-drop-neutral\), var\(--pipeline-ember\)\);/
    );
  });
});

describe('the plate tokens', () => {
  const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
  it('declares the plate, the cut and the three dots over their palette keys', () => {
    for (const name of ['plate', 'cut', 'dot-waiting', 'dot-committed', 'dot-expired']) {
      expect(css).toContain(`--pipeline-${name}: var(--c-pipeline-${name});`);
      expect(encoding.tok(name as encoding.PipelineToken)).toBe(`var(--pipeline-${name})`);
    }
  });
});
