import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import userEvent from '@testing-library/user-event';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { latestActivities } from '@/__tests__/msw/fixtures/home';
import { ActivityFeed, FEED_ROWS } from '@/components/home/activity-feed';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { createTestQueryClient } from '@/__tests__/utils/test-utils';
import { QueryClientProvider } from '@tanstack/react-query';
import type { GlobalActivity, Transaction } from '@/lib/api';

const API = '/api/:network/v1';

function tx(i: number): Transaction {
  return {
    hash: `0x${String(i).padStart(4, '0')}${'ef'.repeat(30)}`,
    blockNumber: 8_775_638 - i,
    blockHash: '0xblock',
    index: 1,
    inputsCount: 2,
    outputsCount: 3,
    fee: '10000',
    isCellbase: false,
    timestamp: String(Date.now() - i * 10_000),
  };
}

function serveTransactions(txs: Transaction[]) {
  server.use(
    http.get(`${API}/transactions`, () =>
      HttpResponse.json({
        data: txs,
        total: txs.length,
        limit: 10,
        hasMore: false,
        nextCursor: null,
      })
    )
  );
}

const rows = () => screen.getByTestId('feed-rows').querySelectorAll('[data-feed-row]');

describe('ActivityFeed', () => {
  beforeEach(() => usePipelineHoverStore.getState().clear());
  afterEach(() => usePipelineHoverStore.getState().clear());

  it('opens on Latest activity with real tab buttons and a View all link', async () => {
    render(<ActivityFeed />);
    const tabs = screen.getByRole('group', { name: 'Feed' });
    const activity = within(tabs).getByRole('button', { name: 'Latest activity' });
    const transactions = within(tabs).getByRole('button', { name: 'Transactions' });
    expect(activity).toHaveAttribute('aria-pressed', 'true');
    expect(activity).toHaveClass('border-jade', 'border-b-2', 'h-11');
    expect(transactions).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByRole('link', { name: 'View all →' })).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
    expect(screen.getByTestId('feed-loading').children).toHaveLength(FEED_ROWS);
    await waitFor(() => expect(rows()).toHaveLength(FEED_ROWS));
  });

  it('shows exactly 8 compact two-line rows: tx · block and age, then the summary and the party', async () => {
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(8));
    const first = rows()[0] as HTMLElement;
    const a = latestActivities[0];
    expect(within(first).getByRole('link', { name: '0x01ababab...abab00' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${a.txHash}`
    );
    expect(within(first).getByRole('link', { name: '#20,819,142' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20819142'
    );
    expect(first).toHaveTextContent(/\ds ago/);
    expect(within(first).getByTestId('tx-event-summary')).toHaveTextContent(
      '2 type calls .bit Time Index State · .bit Time Info'
    );
    // The party in neutral ink and its signed CKB change in red.
    const party = within(first).getByRole('link', { name: /^ckb1qzda/ });
    expect(party).toHaveClass('text-text-dim');
    expect(within(first).getByText('-0.00030000 CKB')).toHaveClass('text-negative');
    // No row icons.
    expect(first.querySelector('svg, img')).toBeNull();
    expect(document.querySelector('a a')).toBeNull();
  });

  it('summarises a DAO deposit and a plain transfer with the labels the full rows use', async () => {
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(8));
    const summaries = screen.getAllByTestId('tx-event-summary').map((s) => s.textContent);
    expect(summaries[1]).toBe('DAO Deposit');
    expect(summaries[2]).toBe('CKB Transfer');
    // The deposit has a second party, counted rather than listed.
    expect(rows()[1]).toHaveTextContent('+1 more');
  });

  it('shows token item deltas on the party line', async () => {
    const token: GlobalActivity = {
      ...latestActivities[2],
      participants: [
        {
          ...latestActivities[2].participants[0],
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash: '0xtoken',
              delta: '1200',
              symbol: 'SEAL',
              decimals: 8,
            },
          ],
        },
      ],
    };
    server.use(http.get(`${API}/activities/latest`, () => HttpResponse.json([token])));
    render(<ActivityFeed />);
    const link = await screen.findByRole('link', { name: /SEAL/ });
    expect(link).toHaveAttribute('href', '/mainnet/tokens/0xtoken');
  });

  it('switches to Transactions: the old latest-transactions rows, View all pointing there', async () => {
    const user = userEvent.setup();
    serveTransactions(Array.from({ length: 10 }, (_, i) => tx(i)));
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(8));

    await user.click(screen.getByRole('button', { name: 'Transactions' }));
    expect(screen.getByRole('button', { name: 'Transactions' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
    expect(screen.getByRole('link', { name: 'View all →' })).toHaveAttribute(
      'href',
      '/mainnet/transactions'
    );
    await waitFor(() => expect(rows()).toHaveLength(8));
    const first = rows()[0] as HTMLElement;
    expect(within(first).getByRole('link', { name: '#8,775,638' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/8775638'
    );
    expect(first).toHaveTextContent('2 in → 3 out');
  });

  it('cross-highlights a transaction row with the pipeline and hovers it back', async () => {
    const user = userEvent.setup();
    const txs = [tx(1), tx(2)];
    serveTransactions(txs);
    render(<ActivityFeed />);
    await user.click(screen.getByRole('button', { name: 'Transactions' }));
    await waitFor(() => expect(rows()).toHaveLength(2));
    const row = document.querySelector(`[data-tx-row="${txs[1].hash}"]`) as HTMLElement;
    expect(row).toHaveAttribute('data-highlighted', 'false');

    act(() =>
      usePipelineHoverStore.getState().setHover({ block: null, tx: txs[1].hash, window: null })
    );
    expect(row).toHaveAttribute('data-highlighted', 'true');
    act(() => usePipelineHoverStore.getState().clearHover());
    act(() => usePipelineHoverStore.getState().togglePin(txs[1].hash));
    expect(row).toHaveAttribute('data-highlighted', 'true');
    act(() => usePipelineHoverStore.getState().clearPin());

    fireEvent.pointerEnter(row);
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverTx: txs[1].hash,
      hoverBlock: txs[1].blockNumber,
      windowBlock: txs[1].blockNumber,
    });
    fireEvent.pointerLeave(row);
    expect(usePipelineHoverStore.getState().hoverTx).toBeNull();
  });

  it('takes realtime transaction pushes into the Transactions tab', async () => {
    const user = userEvent.setup();
    serveTransactions([tx(1)]);
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <ActivityFeed />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole('button', { name: 'Transactions' }));
    await waitFor(() => expect(rows()).toHaveLength(1));

    // What useRealtimeData does on a new_transaction message.
    const pushed = tx(0);
    act(() =>
      client.setQueryData(['latest-transactions'], (old: { data: Transaction[] }) => ({
        ...old,
        data: [pushed, ...old.data].slice(0, 10),
      }))
    );
    await waitFor(() => expect(rows()).toHaveLength(2));
    expect(rows()[0]).toHaveAttribute('data-feed-row', pushed.hash);
    // The new row flashes.
    expect(rows()[0]).toHaveClass('bg-jade/5');
    expect(rows()[1]).not.toHaveClass('bg-jade/5');
  });

  it('reports a failed load instead of an empty feed', async () => {
    server.use(http.get(`${API}/activities/latest`, () => HttpResponse.json({}, { status: 500 })));
    render(<ActivityFeed />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Failed to load the latest activity/
    );
  });
});
