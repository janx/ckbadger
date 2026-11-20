import { describe, expect, it } from 'vitest';
import {
  headerFieldFloorPx,
  headerFieldRoomPx,
  headerRowForm,
  type HeaderNavEntry,
} from '@/lib/header-fit';

/**
 * The header row's form at its edge (review-fb-tests finding 8): the nav stands in the row while
 * the room it leaves the field is at least the field's floor, `>=`, so a row that leaves exactly the
 * floor keeps the nav and a row a hundredth of a pixel narrower does not. The widths here put the
 * room on the floor: a nav entry at the body step, 8.4 px a glyph (`dao` 25.2, `activities` and its
 * chevron 100, `inventory` and its chevron 91.6, `scripts` 58.8, `charts` 50.4: 326), the wordmark
 * 90, the row's two 32 px gaps, the theme switcher 36 and its 12 px gap; the floor 66 px of insets
 * and `search`, 50.4.
 */

const NAV: HeaderNavEntry[] = [
  { label: 'DAO', menu: false },
  { label: 'Activities', menu: true },
  { label: 'Inventory', menu: true },
  { label: 'Scripts', menu: false },
  { label: 'Charts', menu: false },
];

const hundredth = (n: number) => Math.round(n * 100) / 100;

describe('headerRowForm at the field’s floor', () => {
  it('floors the field at 116.4 px from md', () => {
    expect(hundredth(headerFieldFloorPx(true))).toBe(116.4);
  });

  it.each([
    // md–lg, one network: the nav 326 + 4 × 16 = 390; the row less 90 + 64 + 390 + 12 + 36 = 592
    [708.4, false, null],
    // from lg, one network: the nav 326 + 4 × 24 = 422; less 624
    [740.4, true, null],
    // from lg, two networks: the switcher 117.48 first, 326 + 117.48 + 5 × 24 = 563.48; less 765.48
    [881.88, true, 'mainnet'],
  ] as const)(
    'keeps the nav in a %s px row (from lg: %s, switcher: %s), the field exactly at its floor; the menu under it',
    (row, fromLg, switcher) => {
      const parts = { nav: NAV, switcher, word: 'Blocks' };
      const at = { fromMd: true, fromLg };
      expect(hundredth(headerFieldRoomPx('nav', row, parts, { fromSm: true, fromLg }))).toBe(116.4);
      expect(headerRowForm(row, parts, at)).toBe('nav');
      expect(headerRowForm(row - 0.01, parts, at)).toBe('menu');
      expect(headerRowForm(row - 1, parts, at)).toBe('menu');
      expect(headerRowForm(row + 1, parts, at)).toBe('nav');
    }
  );

  it('is the menu below md however wide the row, and the nav from md before the row is measured', () => {
    const parts = { nav: NAV, switcher: null, word: 'Blocks' };
    expect(headerRowForm(5000, parts, { fromMd: false, fromLg: false })).toBe('menu');
    expect(headerRowForm(null, parts, { fromMd: true, fromLg: false })).toBe('nav');
    expect(headerRowForm(null, parts, { fromMd: false, fromLg: false })).toBe('menu');
  });
});
