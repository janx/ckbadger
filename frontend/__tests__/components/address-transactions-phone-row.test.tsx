import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { AddressTransactions } from '@/components/address/address-history';
import { addressTransactionsFixture } from '@/__tests__/msw/fixtures/address';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { inkOf } from '@/__tests__/utils/ink-role';
import { shortenHash } from '@/lib/shorten';
import { PHONE_LINE_PX } from '@/lib/type-metrics';

// Lane D4 (the phone-rows board, its transactions pane): an address's transactions on a phone are
// the transactions list's row. Line 1 the hash and where it stands (the block and its day, or the
// pool's status and the wait); line 2 what it touched (the scripts by name, the notes the API
// attaches); line 3 quiet, the cells, this address's change and the fee, each after its name. It was
// a list of seven labelled lines, one per table column (PA-08): bytes and cycles leave the phone
// row (the transaction's page, the table, the cards from 560 px).
describe('an address’s transactions on a phone (lane D4)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const [tx] = addressTransactionsFixture;

  it('is the board’s row at 358 px: hash and block · day, what it touched, then the figures', () => {
    atContainerWidth(358);
    render(<AddressTransactions rows={[tx]} />);
    const row = screen.getByTestId('address-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['identity', 'head', shortenHash(tx.txHash, 'tight'), '#18,672,923 · 2026-02-21'],
      ['what', 'body', 'Spore', null],
      ['figures', 'quiet', '7 → 11 cells · this address +4,370 CKB · fee 0.0001243 CKB', null],
    ]);
    const figures = row.querySelector('[data-line="figures"]')!;
    expect(dottedItems(figures)).toEqual([
      '7 → 11 cells',
      'this address +4,370 CKB',
      'fee 0.0001243 CKB',
    ]);
    const labels = Array.from(figures.querySelectorAll('[data-role="figure-label"]'));
    expect(labels.map((l) => l.textContent)).toEqual(['this address', 'fee']);
    for (const label of labels) expect(inkOf(label)).toBe('text-text-dim');
    expect(row.textContent).not.toMatch(/5,597|cycles|Size|Cycles/);
    // the key is a hash, the row's one bright thing
    expect(
      row.querySelector(
        '[data-line="identity"] a[href^="/mainnet/tx/"], [data-line="identity"] a[href^="/tx/"]'
      )
    ).not.toBeNull();
  });

  it('writes a pool transaction’s status and wait where the block stands, its notes on line 2', () => {
    atContainerWidth(358);
    render(
      <AddressTransactions
        rows={[
          {
            ...tx,
            blockNumber: null,
            timestamp: null,
            poolStatus: 'proposed',
            timeAddedToPool: new Date(Date.now() - 120000).toISOString(),
            interpretation: {
              status: 'partial',
              reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
            },
          },
        ]}
      />
    );
    const row = screen.getByTestId('address-tx-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', shortenHash(tx.txHash, 'tight'), 'Proposed · in pool for 2m'],
      ['what', 'partial: input not yet resolvable (0xabc:1) · Spore', null],
      ['figures', '7 → 11 cells · this address +4,370 CKB · fee 0.0001243 CKB', null],
    ]);
  });

  // The board's head(): the tight hash at 14 px (92.4), the 12 px between the parts and, at 13 px,
  // the wider of where a transaction stands: "#99,999,999 · 2026-11-02" (24 characters, 187.2) or
  // the widest pool status with the widest wait under 100 days, "Confirming · in pool for 99d" (28,
  // 218.4): 322.8 of a 360 px phone's 328 px line.
  it('states its first line, 322.8 of 328 px, and draws its widest pool row in it', () => {
    atContainerWidth(PHONE_LINE_PX[360]);
    const T = Date.parse('2026-11-19T00:00:00.000Z');
    vi.useFakeTimers({ toFake: ['Date'], now: T });
    try {
      render(
        <AddressTransactions
          rows={[
            {
              ...tx,
              blockNumber: null,
              timestamp: null,
              poolStatus: 'committed_awaiting_index',
              timeAddedToPool: new Date(T - 99 * 86_400_000).toISOString(),
            },
          ]}
        />
      );
      const row = screen.getByTestId('address-tx-row-mobile');
      expect(phoneLines(row)[0].right).toBe('Confirming · in pool for 99d');
      expect(statedHeadPx(row)).toBe(322.8);
      expect(phoneHeadPx(row, 360)).toBe(statedHeadPx(row));
    } finally {
      vi.useRealTimers();
    }
  });
});
