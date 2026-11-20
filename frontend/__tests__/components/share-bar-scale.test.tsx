import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import { TokenHolders } from '@/components/asset/token-holders';
import { CollectionHolders } from '@/components/asset/collection-holders';
import DaoDepositorsPage from '@/app/dao/depositors/page';
import { api, type CollectionHolder } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';

vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));

// PB-11 / ruling R7 (UI review 2026-11-14): a share bar has one scale in both forms and on every
// holder list — the desktop's rounded maximum (the smallest round value at or above the page's
// largest), marked on its axis. A phone bar is no longer full at 10 %, and its track has one fixed
// width, so every track of a list ends at one x.

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const percent = (el: Element) => Number.parseFloat(el.getAttribute('data-share-bar')!);

/** Each phone row's fill and its track, in list order. */
function phoneBars() {
  const rows = screen.getAllByTestId('holder-row-mobile');
  return rows.map((row) => {
    const fill = row.querySelector('[data-share-bar]')!;
    return { row, fill, track: fill.parentElement as HTMLElement };
  });
}

const holder = (n: number, balance: string) => ({
  lockScriptHash: `0x${String(n).repeat(64)}`,
  address: null,
  balance,
  lockCodeHash: `0x${'9b'.repeat(32)}`,
  lockKind: 'Godwoken Custodian Lock',
});

describe('token holders (PB-11)', () => {
  function serve() {
    // USDC's three largest holders: 52.73 %, 26.45 % and 7.76 % of the supply.
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue(
      pagedRead({
        data: [holder(1, '5273'), holder(2, '2645'), holder(3, '776')],
        total: 3,
        limit: 10,
        hasMore: false,
        nextCursor: null,
        totalSupply: '10000',
        anchor: null,
        firstRank: 1,
      })
    );
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
  }

  it('draws 52.73 % and 26.45 % in their ratio on a phone, on the rounded 60 % scale', async () => {
    atContainerWidth(358);
    serve();
    render(<TokenHolders typeHash="0xabc" symbol="USDC" decimals={0} />);
    await screen.findAllByTestId('holder-row-mobile');
    const bars = phoneBars();
    expect(bars.map((b) => percent(b.fill))).toEqual([87.88, 44.08, 12.93]);
    expect(percent(bars[0].fill) / percent(bars[1].fill)).toBeCloseTo(5273 / 2645, 2);
    // The axis mark rides over the first row's track: 0 at its start, the scale at its end.
    const axis = bars[0].row.querySelector('[data-share-axis]')!;
    expect(axis).toHaveTextContent(/^0\s*60%$/);
    expect(bars[1].row.querySelector('[data-share-axis]')).toBeNull();
    // One fixed track: every bar ends at one x.
    const widths = new Set(bars.map((b) => b.track.style.width));
    expect(widths.size).toBe(1);
    expect([...widths][0]).toMatch(/^\d+px$/);
    expect(axis.querySelector('[data-share-axis-track]')).toHaveStyle({ width: [...widths][0] });
  });

  it('marks the same 60 % scale on the desktop table', async () => {
    atContainerWidth(1248);
    serve();
    render(<TokenHolders typeHash="0xabc" symbol="USDC" decimals={0} />);
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    expect(within(table).getByRole('columnheader', { name: /Share/ })).toHaveTextContent(
      /Share\s*0\s*60%/
    );
  });
});

describe('DAO depositors (PB-11)', () => {
  it('draws a 25 % and a 12.5 % depositor in their ratio on the rounded 30 % scale', async () => {
    atContainerWidth(358);
    const row = (rank: number, sharePercent: string) => ({
      rank,
      lockScriptHash: '0x' + String(rank).padStart(64, '0'),
      address: null,
      capacity: '100000000',
      depositCount: 1,
      sharePercent,
    });
    vi.spyOn(api, 'getDaoDepositors').mockResolvedValue(
      pagedRead({
        anchor: { blockNumber: 1, blockHash: '0x' + 'a'.repeat(64) },
        total: 2,
        limit: 50,
        hasMore: false,
        nextCursor: null,
        data: [row(1, '25.00'), row(2, '12.50')],
      })
    );
    render(<DaoDepositorsPage />);
    await screen.findAllByTestId('holder-row-mobile');
    const bars = phoneBars();
    expect(bars.map((b) => percent(b.fill))).toEqual([83.33, 41.67]);
    expect(bars[0].row.querySelector('[data-share-axis]')).toHaveTextContent(/^0\s*30%$/);
  });
});

describe('collection holders (PB-11)', () => {
  it('draws item counts on the rounded scale of the page, not "largest = full"', async () => {
    atContainerWidth(358);
    const holders: CollectionHolder[] = [68, 59, 53].map((itemCount, i) => ({
      lockScriptHash: `0x${String(i + 1).repeat(64)}`,
      address: null,
      itemCount,
      lockCodeHash: null,
      lockKind: null,
    }));
    const load = vi.fn().mockResolvedValue(
      pagedRead({
        data: holders,
        limit: 10,
        total: 3,
        hasMore: false,
        nextCursor: null,
        firstRank: 1,
      })
    );
    render(<CollectionHolders load={load} queryKey="bit" assetClass="identities" />);
    await screen.findAllByTestId('holder-row-mobile');
    const bars = phoneBars();
    // 68 at most: the scale is 75, as the table's Items column marks it.
    expect(bars.map((b) => percent(b.fill))).toEqual([90.67, 78.67, 70.67]);
    expect(bars[0].row.querySelector('[data-share-axis]')).toHaveTextContent(/^0\s*75$/);
  });
});
