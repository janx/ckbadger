import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import userEvent from '@testing-library/user-event';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { feedSamples, latestActivities } from '@/__tests__/msw/fixtures/home';
import { MINUS } from '@/lib/chart-ticks';
import { ActivityFeed, FEED_ROWS, formatOccupiedDelta } from '@/components/home/activity-feed';
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
    expect(activity).toHaveClass('border-jade', 'border-b-2', 'h-11', 'whitespace-nowrap');
    // 14 px tabs on a phone so both tabs and View all fit one line.
    expect(activity).toHaveClass('text-sm', 'sm:text-[15px]');
    expect(transactions).toHaveClass('whitespace-nowrap');
    expect(screen.getByRole('link', { name: 'View all →' })).toHaveClass('whitespace-nowrap');
    expect(transactions).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByRole('link', { name: 'View all →' })).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
    expect(screen.getByTestId('feed-loading').children).toHaveLength(FEED_ROWS);
    await waitFor(() => expect(rows()).toHaveLength(FEED_ROWS));
  });

  it('shows 8 activities: the narrative left, the raw evidence quiet on the right', async () => {
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(8));
    const first = rows()[0] as HTMLElement;
    const a = latestActivities[0];
    expect(first.tagName).toBe('ARTICLE');
    expect(within(first).getByText('Script call')).toHaveAttribute('data-role', 'headline');
    const evidence = first.querySelector('[data-role="evidence"]') as HTMLElement;
    expect(evidence).toHaveClass('text-text-dim');
    expect(within(evidence).getByRole('link', { name: '#20,819,142' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20819142'
    );
    expect(within(evidence).getByRole('link', { name: '0x01ababab...abab00' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${a.txHash}`
    );
    expect(evidence).toHaveTextContent(/\ds ago$/);
    expect(first.querySelector('svg, img')).toBeNull();
    expect(document.querySelector('a a')).toBeNull();
  });

  it('writes calls in call syntax with their args, no count prefix, no gold', async () => {
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(8));
    const first = rows()[0] as HTMLElement;
    const type = first.querySelector('[data-role="detail"]') as HTMLElement;
    expect(type).toHaveTextContent('type.bit Time Index State(0x00) · .bit Time Info(0x00)');
    expect(first).not.toHaveTextContent(/type calls/);
    // A line breaks between calls, never inside one.
    const call = within(type).getByRole('link', { name: '.bit Time Info' })
      .parentElement as HTMLElement;
    expect(call).toHaveClass('whitespace-nowrap');
    expect(call).toHaveTextContent(/^\.bit Time Info\(0x00\)$/);
    expect(within(type).getByRole('link', { name: '.bit Time Info' })).toHaveClass('text-text');
    expect(first.querySelector('[class*="gold"]')).toBeNull();
  });

  it('lists every participant of a DAO withdraw request, the named zero party too', async () => {
    server.use(
      http.get(`${API}/activities/latest`, () =>
        HttpResponse.json([feedSamples.daoWithdrawRequest])
      )
    );
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(1));
    const row = rows()[0] as HTMLElement;
    expect(within(row).getByText('DAO Withdraw Request')).toBeInTheDocument();
    const note = row.querySelector('[data-role="detail"]') as HTMLElement;
    expect(note).toHaveTextContent('26,050.00000000 CKB · deposited at #19,695,550');
    // Detail lines are plain ink, quieter than the headline.
    expect(note).toHaveClass('text-text');
    // Paid first, received next, the zero last.
    const parties = [...row.querySelectorAll('[data-party]')].map((e) => e.textContent);
    expect(parties).toEqual([
      expect.stringMatching(/hxw6h7$/),
      expect.stringMatching(/r4p74p$/),
      expect.stringMatching(/fxdtl8$/),
    ]);
    const amounts = [
      ...row.querySelector('[data-role="participants"]')!.querySelectorAll('[data-ckb-amount]'),
    ].map((e) => e.textContent);
    expect(amounts).toEqual([`${MINUS}412.32488212 CKB`, '+412.32486750 CKB', '0.00000000 CKB']);
    // Occupied only where it changed: two of three.
    expect([...row.querySelectorAll('[data-role="occupied"]')].map((e) => e.textContent)).toEqual([
      `occupied ${MINUS}61`,
      'occupied +61',
    ]);
    expect(row).not.toHaveTextContent('+1 more');
  });

  it('ghosts trailing zeros and right-aligns every amount in one column, so decimals line up', async () => {
    server.use(
      http.get(`${API}/activities/latest`, () => HttpResponse.json([feedSamples.fiberClose]))
    );
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(1));
    const row = rows()[0] as HTMLElement;
    const cells = [
      ...row.querySelector('[data-role="participants"]')!.querySelectorAll('[data-ckb-amount]'),
    ].map((e) => e.parentElement as HTMLElement);
    for (const cell of cells) expect(cell).toHaveClass('col-start-2', 'text-right');
    const amounts = [
      ...row.querySelector('[data-role="participants"]')!.querySelectorAll('[data-ckb-amount]'),
    ];
    for (const a of amounts) expect(a.textContent).toMatch(/\.\d{8} CKB$/);
    expect(amounts[0]).toHaveTextContent(`${MINUS}750.00000000 CKB`);
    expect(amounts[0].querySelector('[data-ghost]')).toHaveTextContent(/^\.00000000$/);
    expect(amounts[0].querySelector('[data-ghost]')).toHaveClass('text-text-ghost');
    expect(amounts[2].querySelector('[data-ghost]')).toHaveTextContent(/^$/);
    expect(row.querySelector('[data-role="participants"]')?.className).toContain(
      'minmax(150px,max-content)'
    );
  });

  it('reads a Fiber close: the action, its channel and capacity, and all three parties', async () => {
    server.use(
      http.get(`${API}/activities/latest`, () => HttpResponse.json([feedSamples.fiberClose]))
    );
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(1));
    const row = rows()[0] as HTMLElement;
    expect(within(row).getByText('fiber · Channel Close')).toBeInTheDocument();
    expect(row.querySelector('[data-role="detail"]')).toHaveTextContent(
      'channel 0x0d3870bb...6e26:0 · 750.00000000 CKB'
    );
    expect(
      [
        ...row.querySelector('[data-role="participants"]')!.querySelectorAll('[data-ckb-amount]'),
      ].map((e) => e.textContent)
    ).toEqual([`${MINUS}750.00000000 CKB`, '+499.99999456 CKB', '+250.00000006 CKB']);
  });

  it("reads an object transfer: its calls and each party's object arriving or departing", async () => {
    server.use(
      http.get(`${API}/activities/latest`, () => HttpResponse.json([feedSamples.objectTransfer]))
    );
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(1));
    const row = rows()[0] as HTMLElement;
    expect(within(row).getByText('Object transfer')).toBeInTheDocument();
    const details = [...row.querySelectorAll('[data-role="detail"]')].map((e) => e.textContent);
    expect(details).toEqual(['typeCKBFS(0xcd6e...2500)', 'lockNervape Shadow Lock(0x0131...5d74)']);
    expect(row).toHaveTextContent('object 0x51c75a...96f2 departed');
    expect(row).toHaveTextContent('object 0x51c75a...96f2 arrived');
    // A party with no item change is still listed.
    expect(row.querySelectorAll('[data-party]')).toHaveLength(3);
  });

  it('reads a token transfer: exact amounts with symbol, and an unnamed lock call by its hash', async () => {
    server.use(
      http.get(`${API}/activities/latest`, () => HttpResponse.json([feedSamples.tokenTransfer]))
    );
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(1));
    const row = rows()[0] as HTMLElement;
    expect(within(row).getByText('Token transfer')).toBeInTheDocument();
    expect(row.querySelector('[data-role="detail"]')).toHaveTextContent(
      'lock0x43b2924e(0x4b00...838e)'
    );
    expect(
      within(row).getByRole('link', { name: `${MINUS}36,583.46853885 Seal` })
    ).toBeInTheDocument();
    expect(within(row).getByRole('link', { name: '+36,583.46853885 Seal' })).toBeInTheDocument();
    expect([...row.querySelectorAll('[data-party]')].map((e) => e.textContent)).toEqual([
      expect.stringMatching(/j3pkeh$/),
      expect.stringMatching(/3fzxu7$/),
    ]);
  });

  it('over six participants, shows the five largest and expands to all and back', async () => {
    const user = userEvent.setup();
    server.use(
      http.get(`${API}/activities/latest`, () => HttpResponse.json([feedSamples.eightParties]))
    );
    render(<ActivityFeed />);
    await waitFor(() => expect(rows()).toHaveLength(1));
    const row = rows()[0] as HTMLElement;
    const parties = () =>
      [...row.querySelectorAll('[data-party]')].map((e) => e.textContent?.slice(-2));
    // The five largest |Δ| (900, 700, 500, 300, 200), paid first.
    expect(parties()).toEqual(['p4', 'p6', 'p2', 'p5', 'p7']);
    const more = within(row).getByRole('button', { name: '+3 participants' });
    expect(more).toHaveAttribute('aria-expanded', 'false');

    await user.click(more);
    expect(parties()).toEqual(['p4', 'p6', 'p1', 'p8', 'p2', 'p5', 'p7', 'p3']);
    const fewer = within(row).getByRole('button', { name: 'Show fewer participants' });
    expect(fewer).toHaveAttribute('aria-expanded', 'true');

    await user.click(fewer);
    expect(parties()).toHaveLength(5);
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
    expect(first.querySelector('[data-role="headline"]')).toHaveTextContent('2 inputs → 3 outputs');
    // The same row grammar: primary info left, the evidence quiet on the right.
    expect(first.querySelector('[data-role="evidence"]')).toHaveTextContent(
      /#8,775,638 · 0x0000efef\.\.\.efefef · /
    );
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

describe('formatOccupiedDelta', () => {
  it('prints occupied capacity in whole CKB (it is bytes), and exactly if it ever is not', () => {
    expect(formatOccupiedDelta('-6100000000')).toBe(`${MINUS}61`);
    expect(formatOccupiedDelta('32000000000')).toBe('+320');
    expect(formatOccupiedDelta('150000001')).toBe('+1.50000001');
  });
});
