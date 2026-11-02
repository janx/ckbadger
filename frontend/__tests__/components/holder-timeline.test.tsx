import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { HolderTimeline } from '@/components/scripts/holder-timeline';
import type { ScriptFamilyDetail } from '@/lib/api';
import { codeHolders, typeLineage, versionNumbers } from '@/lib/script-lineage';

// Table-rows plan L1-3 (board ScriptDetail, rule T8): "Who can change its code" as one 28 px bar
// from the first deployment to today, a segment per code-cell holder with its name inside where it
// fits, the version ticks and dates above, one reading row and one caption. JoyID: three multisigs.
const family = fixture.family as unknown as ScriptFamilyDetail;
const history = family.history!;
const numbers = versionNumbers(family);
const lineage = typeLineage(history)!;
const holders = codeHolders(lineage, history, numbers);
// 2026-11-01 00:00 UTC+8.
const NOW = Date.UTC(2026, 9, 31, 16);

function mount(width: number) {
  stubChartWidth(width);
  render(
    <HolderTimeline
      family={family}
      lineage={lineage}
      holders={holders}
      numbers={numbers}
      now={NOW}
    />
  );
}
const bar = () =>
  screen.getByRole('group', { name: /^Who held JoyID's code cell over time: / }) as HTMLElement;
const segments = () => [...bar().querySelectorAll<HTMLElement>('[data-holder]')];
const reading = () => screen.getByRole('status', { name: 'Code cell holder reading' });

describe('HolderTimeline (table-rows L1-3)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('draws one 28 px segment per holder, oldest first, sized by how long each held the code cell', () => {
    mount(1248);
    expect(bar()).toHaveClass('h-[28px]');
    expect(segments().map((s) => s.dataset.holder)).toEqual([
      '0xd1dbde9214a1cb1e377c9b643ae5c680de0cb95c',
      '0x07a21dc2d33dca6604c5056d54c6f8ce429d68ca',
      '0x65983f1d7520543a6f182a50354fb4ae2deed696',
    ]);
    const start = holders[0].from.timestamp;
    const share = (from: number, until: number) => ((until - from) / (NOW - start)) * 100;
    const [first, second, current] = segments();
    expect(Number.parseFloat(first.style.flexBasis)).toBeCloseTo(
      share(start, holders[1].from.timestamp),
      2
    );
    expect(Number.parseFloat(second.style.flexBasis)).toBeCloseTo(
      share(holders[1].from.timestamp, holders[2].from.timestamp),
      2
    );
    // The holder now takes the rest, and is marked with the left rule.
    expect(current.style.flexGrow).toBe('1');
    expect(current.dataset.current).toBe('true');
    expect(current).toHaveClass('border-l-[3px]');
    expect(first.dataset.current).toBe('false');
    expect(first).not.toHaveClass('border-l-[3px]');
    expect(bar().getAttribute('aria-label')).toBe(
      "Who held JoyID's code cell over time: multisig 0xd1db…b95c from 2023-10-07 to 2025-08-28, multisig 0x07a2…68ca to 2025-11-21, multisig 0x6598…d696 since then"
    );
  });

  it('writes each name inside its segment where it fits, shorter where it is narrow', () => {
    mount(1248);
    const [first, second, current] = segments();
    expect(first).toHaveTextContent(/^multisig 0xd1db…b95c · 3 of 5 signatures$/);
    // 7 % of the bar: the lock args only, or nothing; never the long name cut off.
    expect(['0x07a2…68ca', '']).toContain(second.textContent);
    expect(current).toHaveTextContent(/^multisig 0x6598…d696 · holds the code now$/);
  });

  it('gives way to shorter names on a phone, the title keeping the whole', () => {
    mount(358);
    const [first, second, current] = segments();
    expect(first.textContent).not.toContain('signatures');
    expect(first.textContent!.length).toBeGreaterThan(0);
    expect(second.textContent).toBe('');
    expect(current.textContent).not.toContain('holds the code now');
    for (const s of segments()) expect(s.title.length).toBeGreaterThan(40);
  });

  it('keeps every holder fact in its segment title: dates, signing, versions signed, keys and the keys kept', () => {
    mount(1248);
    const [first, second, current] = segments();
    expect(first.title).toBe(
      'multisig 0xd1dbde…b95c · 3 of 5 signatures · signed v2 and v3 · 2023-10-07 → 2025-08-28 · lock args 0xd1dbde9214a1cb1e377c9b643ae5c680de0cb95c · keys ' +
        holders[0].signing!.keyHashes.join(', ')
    );
    expect(second.title).toContain('signed v4 and v5 · 2025-08-28 → 2025-11-21');
    expect(second.title).toContain(
      'kept 2 of 5 keys of multisig 0xd1db…b95c at the handover on 2025-08-28'
    );
    expect(current.title).toBe(
      'multisig 0x65983f…d696 · signers not on chain yet · has never signed · since 2025-11-21 · lock args 0x65983f1d7520543a6f182a50354fb4ae2deed696 · its keys and threshold appear on chain the first time it signs'
    );
  });

  it('marks the version days above the bar, a second row where two labels would touch', () => {
    mount(900);
    const ticks = [...document.querySelectorAll<HTMLElement>('[data-version-tick]')];
    expect(ticks.map((t) => t.textContent)).toEqual([
      'v1 · v2 2023-10-07',
      'v3 · v4 2025-08-28',
      'v5 2025-11-21',
    ]);
    expect(ticks.map((t) => t.dataset.row)).toEqual(['0', '0', '1']);
    expect(ticks[0].style.left).toBe('0%');
  });

  it('reads the holder now in one row, and the hovered or tapped holder instead', () => {
    mount(1248);
    expect(reading()).toHaveTextContent(
      /^held now by multisig 0x6598…d696\s*since 2025-11-21\s*signers not on chain yet · has never signed\s*3 holders · 5 versions · 2 handovers\s*tap or hover$/
    );
    expect(within(reading()).getByText('multisig 0x6598…d696')).toHaveClass('text-text-bright');
    const [first] = segments();
    fireEvent.mouseEnter(first);
    expect(reading()).toHaveTextContent(
      /^held by multisig 0xd1db…b95c\s*2023-10-07 → 2025-08-28\s*3 of 5 signatures · signed v2 and v3\s*3 holders · 5 versions · 2 handovers/
    );
    fireEvent.mouseLeave(first);
    expect(reading()).toHaveTextContent(/^held now by multisig 0x6598…d696/);
    fireEvent.click(segments()[1]);
    expect(reading()).toHaveTextContent(/^held by multisig 0x07a2…68ca/);
  });

  it('closes with one caption of at most 160 characters', () => {
    mount(1248);
    const caption = document.querySelector('[data-role="caption"]') as HTMLElement;
    expect(caption).toHaveTextContent(
      /^Whoever can spend the code cell can put new code in it; every JoyID cell runs the new code from the next block\.$/
    );
    expect(caption.textContent!.length).toBeLessThanOrEqual(160);
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    // The axis: the first deployment and today.
    expect(screen.getByTestId('holder-axis')).toHaveTextContent(/^2023-10-07\s*today$/);
  });
});

// Regression (2026-11-02, Default Lock): an unspendable holder's lock args are "0x"; its segment
// must not hand them to shortHex ("Not a hex value: 0x").
describe('HolderTimeline · an unspendable holder', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('names it "an unspendable lock" and never shortens its empty lock args', () => {
    const marker = {
      ...lineage[0],
      custody: {
        class: 'unspendable' as const,
        lockCodeHash: `0x${'00'.repeat(32)}`,
        lockHashType: 'data',
        lockArgs: '0x',
        threshold: null,
        keyCount: null,
        requireFirstN: null,
        keyHashes: [],
        verifiedByTx: null,
        witnessState: 'not_applicable' as const,
        owners: [],
      },
    };
    const only = codeHolders([marker], [marker], numbers);
    stubChartWidth(1248);
    render(
      <HolderTimeline
        family={family}
        lineage={[marker]}
        holders={only}
        numbers={numbers}
        now={NOW}
      />
    );
    const [segment] = segments();
    expect(segment).toHaveTextContent(/^an unspendable lock · holds the code now$/);
    expect(segment.title).toContain('an unspendable lock · no upgrade signed · since 2023-10-07');
  });
});
