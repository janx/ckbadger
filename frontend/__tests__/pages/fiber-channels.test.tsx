import { afterEach, describe, expect, it, vi } from 'vitest';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, waitFor, fireEvent, within } from '../utils/test-utils';
import FiberChannelsPage from '@/app/fiber/channels/client-page';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';
import network from '../fixtures/fiber-network.json';
import channel from '../fixtures/fiber-open.json';
import closedChannel from '../fixtures/fiber-force-closed.json';

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const page = (rows: unknown[]) => ({
  data: rows,
  total: rows.length,
  limit: 50,
  hasMore: false,
  nextCursor: null,
});

describe('FiberChannelsPage', () => {
  it('does not show a stale Open row beneath a newly selected filter while it loads', async () => {
    let releaseAll: (() => void) | undefined;
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', async ({ request }) => {
        const filter = new URL(request.url).searchParams.get('state');
        if (filter === null)
          await new Promise<void>((resolve) => {
            releaseAll = resolve;
          });
        return HttpResponse.json(page([filter === 'open' ? channel : closedChannel]));
      })
    );
    render(<FiberChannelsPage />);
    expect(await screen.findByTitle(channel.channelId)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^All/ }));
    await waitFor(() => expect(releaseAll).toBeTypeOf('function'));
    expect(screen.queryByTitle(channel.channelId)).not.toBeInTheDocument();
    releaseAll?.();
    expect(await screen.findByTitle(closedChannel.channelId)).toBeInTheDocument();
  });

  it('leads with the board hero: open now, channels ever, parties, typical lifetime (FF-F5, FF-F9)', async () => {
    server.use(
      http.get('/api/:network/v1/fiber/network', () =>
        HttpResponse.json({
          ...network,
          summary: {
            ...network.summary,
            channelsByToken: [
              { udtTypeHash: `0x${'7f'.repeat(32)}`, symbol: 'USDI', channels: 1, openChannels: 0 },
            ],
          },
        })
      ),
      http.get('/api/:network/v1/fiber/channels', () => HttpResponse.json(page([channel])))
    );
    render(<FiberChannelsPage />);
    expect(screen.getByRole('heading', { level: 1, name: 'Fiber channels' })).toHaveClass(
      'text-index'
    );
    expect(
      screen.getByText(/Only opening and closing a channel touch the chain/)
    ).toBeInTheDocument();
    // no jade uppercase eyebrow in tier 1 (FF-F6)
    expect(screen.queryByText(/Payment channels · on-chain view/)).toBeNull();
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(4));
    expect(screen.getAllByRole('term').map((t) => t.textContent)).toEqual([
      'Open now',
      'Channels ever',
      'Parties',
      'Typical lifetime',
    ]);
    const hero = screen.getAllByRole('term')[0].closest('dl')!;
    expect(within(hero).getByText('since 2023-11-15 · 1 in USDI')).toBeInTheDocument();
    expect(within(hero).getByText('in 1 pair')).toBeInTheDocument();
    expect(within(hero).getByText('1 m 40 s')).toBeInTheDocument();
    expect(within(hero).getByText('2 of 2 closed within an hour')).toBeInTheDocument();
    expect(
      Array.from(hero.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['3,000.00000000 CKB']);
    expect(screen.queryByText(/0 d 0 h/)).toBeNull();
  });

  it('defaults the table to Open, sorted by capacity, with state counts and the board columns (FF-F4, FF-F6, N5)', async () => {
    const requested: string[] = [];
    const sorts: (string | null)[] = [];
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', ({ request }) => {
        const params = new URL(request.url).searchParams;
        requested.push(params.get('state') ?? 'all');
        sorts.push(params.get('sort'));
        return HttpResponse.json({
          ...page([channel]),
          total: 38,
          hasMore: true,
          nextCursor: '1:0x01',
        });
      })
    );
    render(<FiberChannelsPage />);
    expect(await screen.findByRole('heading', { name: 'Channels' })).toHaveClass('text-section');
    await waitFor(() => expect(requested[0]).toBe('open'));
    const open = await screen.findByRole('button', { name: 'Open 1' });
    expect(open).toHaveAttribute('aria-pressed', 'true');
    for (const name of [
      'Closed together 1',
      'Force-closed, settled 0',
      'Force-closed, not settled 1',
      'All 3',
    ])
      expect(screen.getByRole('button', { name })).toHaveAttribute('aria-pressed', 'false');
    const table = await screen.findByRole('table', { name: 'Open channels' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Channel', 'Parties', 'Capacity', 'Opened', 'Open for']);
    expect(within(table).getByText('2023-11-15')).toBeInTheDocument();
    expect(within(table).getByText('3 m 20 s')).toBeInTheDocument();
    expect(within(table).getByText('0xaaaaaaaa…aaaaaa')).toBeInTheDocument();
    expect(sorts.every((sort) => sort === 'capacity')).toBe(true);
    expect(screen.getByText('sorted by capacity')).toBeInTheDocument();
    expect(screen.getByText(/Showing 1-1 of 38 channels/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Closed together 1' }));
    await waitFor(() => expect(requested).toContain('cooperativelyClosed'));
    expect(
      await screen.findByRole('table', { name: 'Closed together channels' })
    ).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'Lifetime' })).toBeInTheDocument();
  });
});

// Board L8 (responsive plan, L3): three labelled lines on a phone, two at 640.
describe('Fiber channel phone row (board L8)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const serve = () =>
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', () =>
        HttpResponse.json(
          page([
            {
              ...channel,
              observedParties: [
                {
                  ...channel.observedParties[0],
                  address: `ckb1qz${'q'.repeat(30)}pr3prn`,
                  lockScriptName: 'Default Lock',
                },
                channel.observedParties[1],
              ],
            },
          ])
        )
      )
    );

  it('labels the date and the duration and names the pair by address suffix at 358 px', async () => {
    atContainerWidth(358);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    expect(screen.queryByRole('table', { name: 'Open channels' })).toBeNull();
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['channel', 'head', '0x01010101…010101', 'open'],
      ['pair', 'body', '…pr3prn ⇄ …bbbbbb Default Lock · unnamed lock', '3,000.00000000 CKB'],
      [
        'time',
        'quiet',
        expect.stringMatching(/^opened 2023-11-15 \d\d:\d\d · open for 3 m 20 s$/),
        null,
      ],
    ]);
    expect(within(row).getByTitle(channel.channelId)).toHaveAttribute(
      'href',
      `/mainnet/fiber/channels/${channel.channelId}`
    );
  });

  it('folds into two lines at 608 px: id and pair with the capacity, then time and state', async () => {
    atContainerWidth(608);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      [
        'channel',
        '0x01010101…010101 · …pr3prn ⇄ …bbbbbb Default Lock · unnamed lock',
        '3,000.00000000 CKB',
      ],
      ['time', expect.stringMatching(/^opened 2023-11-15 .+ · open for 3 m 20 s$/), 'open'],
    ]);
  });
});
