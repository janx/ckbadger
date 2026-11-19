import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import {
  AssetHistory,
  HISTORY_PLOT_HEIGHT,
  heldStory,
  type HeldDay,
} from '@/components/asset/asset-history';
import { ScriptHistoryFigure } from '@/components/scripts/script-family-board';

// Readability plan 2026-11-18, ruling R7 (lane p-assets): the "CKB held" history stays in tier 1,
// 160 px tall instead of 280 (300 on a script page), under one sentence that states what the
// series did, computed from the same exact points ("unchanged at 1,090,672 CKB since 2025-01-07").
// The review (assets-a, assets-b): the flat or spiking plot was the largest, brightest thing of the
// first screen and never said its conclusion.

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const CKB = BigInt(100_000_000);
const day = (date: string, ckb: number | bigint): HeldDay => ({
  date,
  held: BigInt(ckb) * CKB,
});

describe('what a CKB-held series did, from its own points', () => {
  it('says a series that never moved is unchanged since its first day', () => {
    expect(
      heldStory([
        day('2025-01-07', 1_090_672),
        day('2025-01-08', 1_090_672),
        day('2025-01-09', 1_090_672),
      ])
    ).toEqual({ kind: 'flat', held: BigInt(1_090_672) * CKB, since: '2025-01-07' });
  });

  it('says a one-day series is that day', () => {
    expect(heldStory([day('2025-01-07', 215)])).toEqual({
      kind: 'one',
      day: day('2025-01-07', 215),
    });
  });

  it('names where it started and ended, a peak above both ends, and the day its last value began', () => {
    expect(
      heldStory([
        day('2024-01-01', 100),
        day('2024-01-02', 500),
        day('2024-01-03', 500),
        day('2024-01-04', 200),
        day('2024-01-05', 200),
      ])
    ).toEqual({
      kind: 'moved',
      first: day('2024-01-01', 100),
      last: day('2024-01-05', 200),
      // the first day of the greatest value
      peak: day('2024-01-02', 500),
      since: '2024-01-04',
    });
  });

  it('names no peak where an end is the greatest value, and no "since" when the last day moved', () => {
    expect(
      heldStory([day('2024-01-01', 300), day('2024-01-02', 100), day('2024-01-03', 200)])
    ).toEqual({
      kind: 'moved',
      first: day('2024-01-01', 300),
      last: day('2024-01-03', 200),
      peak: null,
      since: null,
    });
  });

  it('compares the exact shannons, not a cut figure', () => {
    // 1,000.00000001 and 1,000 CKB cut to the same summary figure; the series moved
    const first = { date: '2024-01-01', held: BigInt('100000000001') };
    const last = { date: '2024-01-02', held: BigInt('100000000000') };
    expect(heldStory([first, last]).kind).toBe('moved');
  });

  it('refuses an empty series', () => {
    expect(() => heldStory([])).toThrow(/no days/);
  });
});

const daily = (values: readonly [string, number][]) =>
  values.map(([date, ckb]) => ({
    date,
    capacity: (BigInt(ckb) * CKB).toString(),
    occupiedCapacity: (BigInt(ckb) * CKB).toString(),
    freeCapacity: '0',
  }));

function sentence(): HTMLElement {
  return screen.getByTestId('history-sentence');
}

describe('the asset history: 160 px under its sentence', () => {
  it('draws the held plot 160 px tall', () => {
    expect(HISTORY_PLOT_HEIGHT).toBe(160);
    stubChartWidth(1248);
    render(
      <AssetHistory
        assetClass="objects"
        weeklyNoun="transfers"
        daily={daily([
          ['2025-01-07', 1_090_672],
          ['2025-01-08', 1_090_672],
        ])}
        weekly={[]}
        completeThrough="2025-01-08"
      />
    );
    const plot = screen.getByRole('img', { name: 'CKB held in its cells, CKB' });
    expect(Number(plot.getAttribute('height'))).toBe(160);
  });

  it('says a flat series in one sentence above the plot, in the summary form', () => {
    stubChartWidth(1248);
    render(
      <AssetHistory
        assetClass="objects"
        weeklyNoun="transfers"
        daily={daily([
          ['2025-01-07', 1_090_672],
          ['2025-01-08', 1_090_672],
          ['2025-01-09', 1_090_672],
        ])}
        weekly={[]}
        completeThrough="2025-01-09"
      />
    );
    expect(sentence()).toHaveTextContent('CKB held: unchanged at 1,090,672 CKB since 2025-01-07.');
    expect(sentence().tagName).toBe('P');
    // above the plot it reads
    const plot = screen.getByRole('img', { name: 'CKB held in its cells, CKB' });
    expect(
      sentence().compareDocumentPosition(plot) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // the reading ink on the prose measure (R3, B7)
    expect(sentence()).toHaveClass('text-body', 'max-w-measure');
    expect(sentence()).not.toHaveClass('text-text-dim');
  });

  it('says where a moving series went, its peak and the day its last value began', () => {
    stubChartWidth(1248);
    render(
      <AssetHistory
        assetClass="tokens"
        weeklyNoun="transactions"
        daily={daily([
          ['2024-01-01', 100],
          ['2024-01-02', 215_000_000],
          ['2024-01-03', 354_013],
          ['2024-01-04', 354_013],
        ]).map((d) => ({ ...d, supply: '1' }))}
        weekly={[]}
        completeThrough="2024-01-04"
        token={{ symbol: 'USDC', decimals: 6 }}
      />
    );
    expect(sentence()).toHaveTextContent(
      'CKB held: rose from 100 CKB on 2024-01-01 to 354,013 CKB, peaking at 215,000,000 CKB on 2024-01-02; unchanged since 2024-01-03.'
    );
  });

  it('says a fall, and a series back where it started', () => {
    stubChartWidth(1248);
    const { unmount } = render(
      <AssetHistory
        assetClass="objects"
        weeklyNoun="transfers"
        daily={daily([
          ['2024-01-01', 2_000],
          ['2024-01-02', 1_200],
        ])}
        weekly={[]}
        completeThrough="2024-01-02"
      />
    );
    expect(sentence()).toHaveTextContent(
      'CKB held: fell from 2,000 CKB on 2024-01-01 to 1,200 CKB.'
    );
    unmount();
    render(
      <AssetHistory
        assetClass="objects"
        weeklyNoun="transfers"
        daily={daily([
          ['2024-01-01', 500],
          ['2024-01-02', 100],
          ['2024-01-03', 500],
          ['2024-01-04', 500],
        ])}
        weekly={[]}
        completeThrough="2024-01-04"
      />
    );
    expect(sentence()).toHaveTextContent(
      'CKB held: back at 500 CKB, as on 2024-01-01; unchanged since 2024-01-03.'
    );
  });

  it('says a one-day series as that day', () => {
    stubChartWidth(1248);
    render(
      <AssetHistory
        assetClass="objects"
        weeklyNoun="transfers"
        daily={daily([['2025-01-07', 215]])}
        weekly={[]}
        completeThrough="2025-01-07"
      />
    );
    expect(sentence()).toHaveTextContent(
      'CKB held: 215 CKB on 2025-01-07, the one complete day so far.'
    );
  });

  it('carries each cut figure’s exact value in its title', () => {
    stubChartWidth(1248);
    render(
      <AssetHistory
        assetClass="objects"
        weeklyNoun="transfers"
        daily={[
          {
            date: '2025-01-07',
            capacity: '109067250000001',
            occupiedCapacity: '0',
            freeCapacity: '109067250000001',
          },
          {
            date: '2025-01-08',
            capacity: '109067250000001',
            occupiedCapacity: '0',
            freeCapacity: '109067250000001',
          },
        ]}
        weekly={[]}
        completeThrough="2025-01-08"
      />
    );
    expect(sentence()).toHaveTextContent('unchanged at 1,090,672 CKB since 2025-01-07');
    expect(sentence().querySelector('[data-ckb-amount]')).toHaveAttribute(
      'title',
      expect.stringContaining('1,090,672.50000001')
    );
  });
});

describe('the script history: 160 px under its sentence', () => {
  it('draws the held plot 160 px tall under the sentence its points give', () => {
    stubChartWidth(1248);
    render(
      <ScriptHistoryFigure
        family={fixture.family as never}
        chart={
          {
            ...(fixture.chart as never as object),
            data: [
              {
                date: '2025-01-07',
                values: {
                  occupiedCapacity: String(600 * 1e8),
                  freeCapacity: String(400 * 1e8),
                  capacity: String(1000 * 1e8),
                },
              },
              {
                date: '2025-01-08',
                values: {
                  occupiedCapacity: String(600 * 1e8),
                  freeCapacity: String(400 * 1e8),
                  capacity: String(1000 * 1e8),
                },
              },
            ],
          } as never
        }
        lineage={null}
        numbers={new Map()}
        active={null}
        onActive={() => {}}
      />
    );
    const [held] = screen.getAllByRole('img');
    expect(Number(held.getAttribute('height'))).toBe(160);
    expect(sentence()).toHaveTextContent('CKB held: unchanged at 1,000 CKB since 2025-01-07.');
    expect(
      sentence().compareDocumentPosition(held) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  // Lane p-assets (the follow-up's item 12): the route states each day's whole, `capacity`; the
  // page reads it and sums nothing, so a day without it is a contract break, named
  it('reads each day’s capacity as the route states it, and refuses a day without it', () => {
    stubChartWidth(1248);
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <ScriptHistoryFigure
          family={fixture.family as never}
          chart={
            {
              ...(fixture.chart as never as object),
              data: [
                {
                  date: '2025-01-07',
                  values: { occupiedCapacity: String(600 * 1e8), freeCapacity: String(400 * 1e8) },
                },
              ],
            } as never
          }
          lineage={null}
          numbers={new Map()}
          active={null}
          onActive={() => {}}
        />
      )
    ).toThrow(/2025-01-07: capacity/);
    error.mockRestore();
  });
});
