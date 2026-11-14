import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { ActivityRow } from '@/components/activity/activity-row';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import type { Transaction } from '@/lib/api';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';

/**
 * PA-22 (UI review 2026-11-14): a block row read "20s ago" while a transaction or an activity of
 * the same seconds read "2026-11-14" — a bare date on a list that is live. A row of a live list
 * (blocks, transactions, activities, the pool) says its age in the one age form, the full date-time
 * in its title; a list that is historical by nature (an address's history, a block's
 * transactions) keeps the date, its age the date's title.
 */
const NOW = Date.parse('2026-11-14T08:00:00Z');
const twentySecondsAgo = new Date(NOW - 20_000).toISOString();

describe('the age on a live row (PA-22)', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('writes an activity of a live feed by its age, the date-time its title', () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(NOW);
    render(
      <ActivityRow activity={{ ...feedSamples.tokenTransfer, timestamp: twentySecondsAgo }} age />
    );
    const age = screen.getByText('20s ago');
    expect(age.tagName).toBe('TIME');
    expect(age).toHaveAttribute('title', '2026-11-14 15:59:40 UTC+8');
    expect(screen.queryByText('2026-11-14')).toBeNull();
  });

  it('keeps the date on a historical list, its age the title', () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(NOW);
    render(
      <ActivityRow activity={{ ...feedSamples.tokenTransfer, timestamp: twentySecondsAgo }} />
    );
    expect(screen.getByText('2026-11-14')).toHaveAttribute('title', '20s ago');
  });

  it('writes a committed transaction by its age in the table and in the phone row', () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(NOW);
    const hash = `0x00de304b${'00'.repeat(25)}25cbbc`;
    const tx: Transaction = {
      hash,
      blockNumber: 20829775,
      blockHash: `0x${'cd'.repeat(32)}`,
      index: 1,
      inputsCount: 3,
      outputsCount: 3,
      fee: '30000',
      value: '0',
      outputCapacity: '100000000000',
      txSize: 903,
      cycles: 1704957,
      cyclesStatus: 'complete',
      timestamp: twentySecondsAgo,
      isCellbase: false,
      activity: {
        ...feedSamples.tokenTransfer,
        txHash: hash,
        blockNumber: 20829775,
        timestamp: twentySecondsAgo,
      },
    };
    for (const width of [1232, 358]) {
      atContainerWidth(width);
      const { unmount } = render(<TransactionListTable transactions={[tx]} />);
      const age = screen.getByText('20s ago');
      expect(age).toHaveAttribute('title', '2026-11-14 15:59:40 UTC+8');
      if (width === 358)
        expect(phoneLines(screen.getByTestId('transaction-row-mobile'))[0].right).toBe(
          '#20,829,775 · 20s ago'
        );
      unmount();
    }
  });
});
