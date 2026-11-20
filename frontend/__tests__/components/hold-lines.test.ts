import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { COMPOSITION_TIER_LABELS } from '@/lib/asset-utils';
import { SHORT_FORMS } from '@/lib/shorten';
import { WIDEST_ROW_COUNT, shortChars } from '@/lib/list-bounds';
import { PHONE_PART_GAP_PX, PHONE_LINE_PX, fontPx, textPx } from '@/lib/type-metrics';
import { PHONE_SHARE_TRACK } from '@/components/ui/share-bar';

// Review of phases C and D, logic SHOULD 4 (lane p-assets): a phone row's held line keeps its right
// part at the line's right edge only where it fits beside the left part's widest unbreakable item;
// elsewhere the right part goes under the left (components/ui/phone-row.tsx, `hold`). Each held
// line of the asset lists, its widest left item and its widest right part, measured at their steps
// (0.6 em a glyph, lib/type-metrics.ts), and where the two share the line of a 360 px phone (328
// px) and of a 390 px one (358 px). A lock's words are the registry's: the longest name the bundled
// registry gives (docs/metadata/scripts), or "lock" and a code hash at the row's size.

const small = (chars: number) => textPx(chars, fontPx('small'));
const meta = (chars: number) => textPx(chars, fontPx('meta'));
const together = (left: number, right: number) => left + PHONE_PART_GAP_PX + right;

/** The registry's lock and script names, from the bundled metadata the API's registry reads. */
function registryNames(): string[] {
  const dir = join(__dirname, '..', '..', '..', 'docs', 'metadata', 'scripts');
  return readdirSync(dir)
    .filter((f) => f.endsWith('.toml'))
    .flatMap((f) =>
      [...readFileSync(join(dir, f), 'utf8').matchAll(/^name\s*=\s*"([^"]+)"/gm)].map((m) => m[1])
    );
}
const longest = (names: readonly string[]) =>
  names.reduce((a, b) => (b.length > a.length ? b : a), '');

/** A composition badge at its small size: the off-chain form (mark, 6 px, words at 12 px). */
const offChainBadge = (label: string) => 12 + 6 + meta(label.length);
/** The on-chain form: its sides (4 and 6 px), its 1 px frame, the mark and its 5 px gap. */
const onChainBadge = (label: string) => 4 + 6 + 2 + 12 + 5 + meta(label.length);

describe('the held lines of the asset lists (logic SHOULD 4): where the right part stays', () => {
  const names = registryNames();
  it('reads the registry', () => {
    expect(names.length).toBeGreaterThan(50);
    // the longest name today; a longer one moves the arithmetic below and fails it
    expect(longest(names)).toBe('Godwoken ETH Account Lock');
  });

  // Re-pinned on purpose (the ink and size roles round): the share is a value, at 13 px, and its
  // quiet line goes to 13 px for every list (the shared lane); the share's 7 ch were 50.4 px at 12
  // (202.4 px, 113.6 beside it at 360, 143.6 at 390). Every comparison below still holds.
  it('a holder’s share line: the lock stays beside the bar to 109.4 px at 360 and 139.4 at 390', () => {
    // the 22 px indent, the track, the 10 px gap and the share in 7 ch at 13 px: one unit
    const left = 22 + PHONE_SHARE_TRACK + 10 + small(7);
    expect(left).toBeCloseTo(206.6, 10);
    expect(PHONE_LINE_PX[360] - PHONE_PART_GAP_PX - left).toBeCloseTo(109.4, 10);
    expect(PHONE_LINE_PX[390] - PHONE_PART_GAP_PX - left).toBeCloseTo(139.4, 10);
    // 14 glyphs of a lock's words at 13 px beside the bar at 360 (109.2), 17 at 390 (132.6)
    expect(Math.floor((PHONE_LINE_PX[360] - PHONE_PART_GAP_PX - left) / small(1))).toBe(14);
    expect(Math.floor((PHONE_LINE_PX[390] - PHONE_PART_GAP_PX - left) / small(1))).toBe(17);
    // the lock's words at 13 px: the registry's longest name, or "lock" and a code hash at the
    // phone's tight size
    const name = small(longest(names).length);
    const byHash = small('lock '.length + shortChars(SHORT_FORMS.tight.hash));
    expect(name).toBeCloseTo(195, 10);
    expect(byHash).toBeCloseTo(124.8, 10);
    // under the bar at both widths for the longest name; by its hash, under at 360, beside at 390
    expect(together(left, name)).toBeGreaterThan(PHONE_LINE_PX[390]);
    expect(together(left, byHash)).toBeGreaterThan(PHONE_LINE_PX[360]);
    expect(together(left, byHash)).toBeLessThanOrEqual(PHONE_LINE_PX[390]);
    // "JoyID" stays beside the bar; "UTXOSwap Proxy Lock" (the review's) goes under
    expect(together(left, small('JoyID'.length))).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
    expect(together(left, small('UTXOSwap Proxy Lock'.length))).toBeGreaterThan(PHONE_LINE_PX[390]);
    // on a line of its own the longest is narrower than the line: it never runs past it
    expect(name).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
  });

  it('a collection item’s holder line: the composition goes under the holder where it is not one of the two on-chain badges', () => {
    // "held by" and the holder at the row size: an address (13), or a lock hash where its address
    // is not indexed (17), one unit at 13 px
    const left = small('held by '.length + shortChars(SHORT_FORMS.row.hash));
    expect(left).toBeCloseTo(195, 10);
    const notRecorded = small('composition not recorded'.length);
    const widestOffChain = offChainBadge(longest(Object.values(COMPOSITION_TIER_LABELS)));
    const widestOnChain = Math.max(
      onChainBadge(COMPOSITION_TIER_LABELS.pure_ckb),
      onChainBadge(COMPOSITION_TIER_LABELS.btc_ckb)
    );
    expect(widestOffChain).toBeCloseTo(140.4, 10);
    expect(widestOnChain).toBeCloseTo(86.6, 10);
    expect(together(left, notRecorded)).toBeGreaterThan(PHONE_LINE_PX[390]);
    expect(together(left, widestOffChain)).toBeGreaterThan(PHONE_LINE_PX[360]);
    expect(together(left, widestOffChain)).toBeLessThanOrEqual(PHONE_LINE_PX[390]);
    expect(together(left, widestOnChain)).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
  });

  it('a cluster object’s holder line: the composition stays beside the holder at both widths', () => {
    // the widest unbreakable item: an unnamed lock, "lock" and its code hash at the row size (a
    // name breaks between its words; "held by" and an address is 21 characters)
    const left = Math.max(
      small('held by '.length + shortChars(SHORT_FORMS.row.address)),
      small('lock '.length + shortChars(SHORT_FORMS.row.hash))
    );
    expect(left).toBeCloseTo(171.6, 10);
    const widest = Math.max(
      offChainBadge(longest(Object.values(COMPOSITION_TIER_LABELS))),
      small('not recorded'.length)
    );
    expect(together(left, widest)).toBeCloseTo(324, 10);
    expect(together(left, widest)).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
  });

  it('an object collection’s line in the inventory: "Composition unavailable" goes under at 360', () => {
    // the widest item of its separated list: a count of eight digits and its longest noun
    const left = small(`${WIDEST_ROW_COUNT} activities`.length);
    expect(left).toBeCloseTo(163.8, 10);
    const unavailable = small('Composition unavailable'.length);
    expect(together(left, unavailable)).toBeCloseTo(355.2, 10);
    expect(together(left, unavailable)).toBeGreaterThan(PHONE_LINE_PX[360]);
    expect(together(left, unavailable)).toBeLessThanOrEqual(PHONE_LINE_PX[390]);
    const badge = offChainBadge(longest(Object.values(COMPOSITION_TIER_LABELS)));
    expect(together(left, badge)).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
  });
});
