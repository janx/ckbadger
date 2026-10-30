import { describe, expect, expectTypeOf, it } from 'vitest';
import {
  forkDetailHref,
  forkEventHref,
  forkRefHref,
  missingAddressWords,
  orphanFateWords,
  parseForkRoute,
  uncleDistanceWords,
  uncleRateFigure,
} from '@/lib/fork-format';
import listFixture from '@/__tests__/fixtures/fork-api-producer/fork-list.json';
import type { ForkFirstOrphanStatus, ReorgEvent } from '@/lib/api';

const event = listFixture.data[0] as unknown as ReorgEvent;

describe('fork routes (D7)', () => {
  it('addresses a fork by its fork point height and the first 8 hex of its first orphan', () => {
    expect(forkDetailHref(20617704, `0x${'a9bc1b75'}${'0'.repeat(56)}`)).toBe(
      '/forks/20617704/a9bc1b75'
    );
    expect(forkEventHref(event)).toBe('/forks/10/17404795');
    expect(() => forkDetailHref(10, '0x1234')).toThrow('not a 32-byte hash');
  });

  it('links from the API’s firstOrphanHash, never from the preview or the ms id (R2b-b)', () => {
    expect(forkEventHref({ ...event, preview: null })).toBe('/forks/10/17404795');
    expect(
      forkEventHref({ ...event, firstOrphanHash: null, firstOrphanStatus: 'not_observed' })
    ).toBeNull();
    expect(() =>
      forkEventHref({ ...event, firstOrphanHash: null, firstOrphanStatus: 'recorded' })
    ).toThrow('fork #10 is recorded but has no first orphan hash');
  });

  it('addresses a related event by its own fork point and first orphan', () => {
    expect(
      forkRefHref({
        forkPointNumber: 20617704,
        firstOrphanHash: `0x9f8e7d6c${'0'.repeat(56)}`,
        firstOrphanStatus: 'recorded',
      })
    ).toBe('/forks/20617704/9f8e7d6c');
    expect(
      forkRefHref({ forkPointNumber: 7, firstOrphanHash: null, firstOrphanStatus: 'not_observed' })
    ).toBeNull();
  });

  it('says why a switch has no detail page', () => {
    expect(missingAddressWords('not_observed')).toBe('nothing was rolled back, so no detail page');
    expect(() => missingAddressWords('recorded')).toThrow(
      'fork first orphan status recorded has an address'
    );
  });

  // contracts W5-2: every automatic switch persists its first orphan; the node walk is gone.
  it('knows only recorded and not_observed first orphans', () => {
    expectTypeOf<ForkFirstOrphanStatus>().toEqualTypeOf<'recorded' | 'not_observed'>();
    for (const gone of ['traced', 'needs_node_reader', 'old_branch_unavailable'])
      expect(() => missingAddressWords(gone as ForkFirstOrphanStatus)).toThrow(
        `Fork: unknown first orphan status ${gone}`
      );
    expect(() =>
      forkRefHref({
        forkPointNumber: 7,
        firstOrphanHash: null,
        firstOrphanStatus: 'traced' as ForkFirstOrphanStatus,
      })
    ).toThrow('Fork: unknown first orphan status traced');
  });

  it('parses only a canonical height and a lowercase 8-hex prefix', () => {
    expect(parseForkRoute('20617704', 'a9bc1b75')).toEqual({
      height: 20617704,
      orphan: 'a9bc1b75',
    });
    for (const [h, o] of [
      ['1754000000000x', 'a9bc1b75'],
      ['020617704', 'a9bc1b75'],
      ['20617704', 'A9BC1B75'],
      ['20617704', 'a9bc1b7'],
      ['20617704', '0xa9bc1b'],
    ])
      expect(parseForkRoute(h, o)).toBeNull();
  });
});

describe('orphanFateWords', () => {
  it('says what became of the dropped block in plain words', () => {
    expect(orphanFateWords('included_as_uncle')).toBe('uncle');
    expect(orphanFateWords('canonical_now')).toBe('back on the main chain');
    expect(orphanFateWords('not_included_as_uncle')).toBe('not taken as an uncle');
    expect(orphanFateWords('uncle_fate_pending')).toBe('uncle inclusion still possible');
    expect(() => orphanFateWords('mystery' as never)).toThrow('unknown orphan fate mystery');
  });
});

describe('uncleRateFigure', () => {
  it('reads the window’s exact uncle rate from the per-day counts against the 2.5% target', () => {
    expect(
      uncleRateFigure({
        days: [{ date: '2026-10-20', canonicalBlocks: 279_561, uncles: 7_178, nodeSwitches: 0 }],
        unclesInWindow: 7_178,
      })
    ).toEqual({ rate: '2.57%', uncles: 7_178, blocks: 279_561 });
    expect(() =>
      uncleRateFigure({
        days: [{ date: '2026-10-20', canonicalBlocks: 10, uncles: 1, nodeSwitches: 0 }],
        unclesInWindow: 2,
      })
    ).toThrow('days hold 1 uncles but the window says 2');
    expect(uncleRateFigure({ days: [], unclesInWindow: 0 })).toBeNull();
  });
});

describe('blockClock', () => {
  it('reads a block header time on the UTC+8 clock with its milliseconds', async () => {
    const { blockClock } = await import('@/lib/fork-format');
    expect(blockClock('2023-11-14T22:13:20.050+00:00')).toBe('06:13:20.050');
    expect(blockClock('2026-10-02T01:12:32.761Z')).toBe('09:12:32.761');
    expect(() => blockClock('yesterday')).toThrow('Invalid block timestamp');
  });
});

describe('uncleDistanceWords', () => {
  it('reads uncle distances the board’s way', () => {
    expect(
      uncleDistanceWords([
        { distance: 1, count: 51 },
        { distance: 2, count: 215 },
      ])
    ).toBe('51 one block later, 215 two blocks later');
    expect(uncleDistanceWords([{ distance: 12, count: 1 }])).toBe('1 12 blocks later');
  });
});
