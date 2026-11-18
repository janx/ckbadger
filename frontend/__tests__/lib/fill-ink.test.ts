import { describe, expect, it } from 'vitest';
import {
  FILL_INKS,
  PALETTE,
  THEMES,
  contrastRatio,
  inkOnColours,
  inkOnFill,
  overGround,
  type PaletteKey,
} from '@/lib/palette';
import { CLASS_KEYS, DATA_KEY } from '@/lib/design-tokens';
import { CONCENTRATION_STEPS } from '@/components/ui/concentration-bar';
import { PROVIDER_OPACITY } from '@/app/network/distributions';

// DY-03 (UI review 2026-11-14): the ink of words inside a filled segment comes from the fill's own
// luminance, per theme. Every bar that writes words on its fills is listed here with its real steps.
const BARS: { name: string; key: PaletteKey; steps: readonly number[] }[] = [
  { name: 'DAO "who deposits" concentration', key: CLASS_KEYS.dao, steps: CONCENTRATION_STEPS },
  { name: 'network "where they run"', key: DATA_KEY, steps: PROVIDER_OPACITY },
  // dao-board.tsx CLAIMABLE_FILL: the claimable part of the requested withdrawals
  { name: 'DAO claimable', key: CLASS_KEYS.dao, steps: [0.45] },
];

describe('overGround', () => {
  it('lays a palette colour over a ground as the eye sees it', () => {
    expect(overGround('dark', 'gold', 1)).toBe(PALETTE.dark.gold);
    expect(overGround('dark', 'gold', 0)).toBe(PALETTE.dark['base-bg']);
    // #e4bf76 at 70 % over #15120f (re-pinned on purpose with the night gold of readability ruling
    // R2; with #c9a45c it was #937845)
    expect(overGround('dark', 'gold', 0.7)).toBe('#a68b57');
    // #80600f at 70 % over the paper #f3eee4
    expect(overGround('light', 'gold', 0.7)).toBe('#a38b4f');
    expect(overGround('light', 'gold', 0.7, 'base-surface')).toBe('#a58d53');
    expect(() => overGround('dark', 'gold', 1.2)).toThrow('alpha 1.2 of gold');
  });
});

// Lane E2 round 4: a label on a gradient band stands on several colours at once; the ink is the
// one whose lowest contrast over them is highest, and one colour is inkOnFill's own case.
describe('inkOnColours', () => {
  it('is inkOnFill for one colour', () => {
    for (const theme of THEMES)
      for (const a of [0.35, 0.68, 1])
        expect(inkOnColours(theme, [overGround(theme, 'object', a, 'base-bg')])).toEqual(
          inkOnFill(theme, 'object', a)
        );
  });

  it('takes the ink that reads best on the worst of several colours', () => {
    const colours = [PALETTE.dark['text-bright'], PALETTE.dark['base-surface']];
    const picked = inkOnColours('dark', colours);
    const worst = (ink: PaletteKey) =>
      Math.min(...colours.map((c) => contrastRatio(PALETTE.dark[ink], c)));
    expect(picked.contrast).toBeCloseTo(Math.max(...FILL_INKS.map(worst)), 10);
    expect(picked.contrast).toBeCloseTo(worst(picked.ink), 10);
    expect(() => inkOnColours('dark', [])).toThrow('an ink on no colour');
  });
});

describe('inkOnFill', () => {
  it('takes the dark ink on a light fill and the light ink on a dark one, in each theme', () => {
    // night: full gold is light, the well's near-black reads on it; at 22 % the fill is dark
    expect(inkOnFill('dark', 'gold', 1).ink).toBe('base-void');
    expect(inkOnFill('dark', 'gold', 0.22).ink).toBe('text-bright');
    // day: full gold is a deep ink, the paper-white well reads on it; at 70 % it is light
    expect(inkOnFill('light', 'gold', 1).ink).toBe('base-void');
    expect(inkOnFill('light', 'gold', 0.7).ink).toBe('text-bright');
    // the same step, a different ink per theme: chosen from the fill, not from the rank
    expect(inkOnFill('dark', 'gold', 0.7).ink).toBe('base-void');
  });

  it('returns the better of the two fill inks and its contrast', () => {
    for (const theme of THEMES)
      for (const a of [0.2, 0.5, 0.8]) {
        const fill = overGround(theme, 'jade', a);
        const { ink, contrast } = inkOnFill(theme, 'jade', a);
        const ratios = FILL_INKS.map((k) => contrastRatio(PALETTE[theme][k], fill));
        expect(contrast).toBe(Math.max(...ratios));
        expect(contrastRatio(PALETTE[theme][ink], fill)).toBe(contrast);
      }
  });

  it.each(THEMES)(
    'reads every step of every labelled bar at 4.5:1 or more in the %s theme, on the page and on a surface',
    (theme) => {
      const short: string[] = [];
      for (const bar of BARS)
        for (const step of bar.steps)
          for (const ground of ['base-bg', 'base-surface'] as const) {
            const { ink, contrast } = inkOnFill(theme, bar.key, step, ground);
            if (contrast < 4.5)
              short.push(`${bar.name} ${step} on ${ground}: ${ink} ${contrast.toFixed(2)}:1`);
          }
      expect(short).toEqual([]);
    }
  );
});
