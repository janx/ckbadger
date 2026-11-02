import { afterEach, describe, expect, it, vi } from 'vitest';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { render, screen, waitFor, fireEvent, within } from '../utils/test-utils';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
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

  it('leads with open now as the one jade figure, the picture under it, and the other figures in one quiet row (FF-F5, FF-F9, R11, R13)', async () => {
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
    const { container } = render(<FiberChannelsPage />);
    const hero = screen.getByRole('region', { name: 'Fiber channels' });
    expect(within(hero).getByRole('heading', { level: 1, name: 'Fiber channels' })).toHaveClass(
      'text-index'
    );
    // R13: one short lede (dim, ≤ 90 characters) and the way down to the explanation in tier 2
    const lede = within(hero).getByText(/^Channels touch the chain only to open and to close\./);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    // one line in its 560 px (about 8.3 px a character at 13 px), as the DAO page's lede
    expect(lede.textContent!.length).toBeLessThanOrEqual(67);
    expect(within(lede).getByRole('link', { name: 'How it works ↓' })).toHaveAttribute(
      'href',
      '#evidence'
    );
    // no jade uppercase eyebrow in tier 1 (FF-F6)
    expect(screen.queryByText(/Payment channels · on-chain view/)).toBeNull();
    // the display figure: open now, in Fiber's jade, its unit quieter, its locked CKB beside it
    const figure = await within(hero).findByText('1');
    expect(figure).toHaveClass('sm:text-display', 'text-jade');
    expect(within(figure).getByText('channel')).toHaveClass('text-hero', 'text-text-dim');
    expect(within(hero).getByText('open now')).toBeInTheDocument();
    const companion = within(hero).getByText(/^locking/);
    expect(
      Array.from(companion.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['3,000.00000000 CKB']);
    // the other three figures: one quiet row under the reading, outside the hero section
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(3));
    expect(screen.getAllByRole('term').map((t) => t.textContent)).toEqual([
      'Channels ever',
      'Parties',
      'Typical lifetime',
    ]);
    const row = screen.getAllByRole('term')[0].closest('dl')!;
    expect(hero.contains(row)).toBe(false);
    expect(
      within(row)
        .getAllByRole('definition')
        .map((dd) => dd.textContent)
    ).toEqual([
      '3 · since 2023-11-15 · 1 in USDI',
      '2 · in 1 pair',
      '1 m 40 s · 2 of 2 closed within an hour',
    ]);
    expect(screen.queryByText(/0 d 0 h/)).toBeNull();
    // R11: the figure is tier 1's only jade text
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const jade = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => /^text-jade(\/\d+)?$/.test(c))
    );
    expect(jade).toEqual([figure]);
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
  });

  it('moves the concept and the graph explainer to tier 2, every sentence kept (F1)', async () => {
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', () => HttpResponse.json(page([channel])))
    );
    const { container } = render(<FiberChannelsPage />);
    const tier2 = await waitFor(() => {
      const el = container.querySelector('[data-tier="2"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });
    expect(container.querySelector('#evidence')!.contains(tier2)).toBe(true);
    expect(tier2).toHaveTextContent(
      'Fiber is a payment-channel network on CKB. Two parties lock CKB, or a token, in one shared cell and pay each other off chain as often as they like. Only opening and closing a channel touch the chain, so that is what CKBadger sees.'
    );
    expect(
      await within(tier2).findByRole('heading', { name: 'How to read the graph' })
    ).toBeInTheDocument();
    expect(tier2).toHaveTextContent(/Each circle is a party: the address that funded/);
    expect(tier2).toHaveTextContent(/Parties are on-chain addresses, not Fiber node ids/);
    // two balanced columns: what the chain sees (with what a party is) | how to read the graph
    const columns = tier2.querySelector(':scope > div')!.children;
    expect(columns).toHaveLength(2);
    expect(columns[0]).toHaveTextContent(/Fiber is a payment-channel network/);
    expect(columns[0]).toHaveTextContent(/Parties are on-chain addresses/);
    expect(columns[1]).toHaveTextContent(/Each circle is a party: the address/);
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    expect(tier1).not.toHaveTextContent(/Fiber is a payment-channel network/);
    expect(tier1).not.toHaveTextContent(/Parties are on-chain addresses/);
    expect(tier1).not.toHaveTextContent(/Each circle is a party: the address/);
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
    // a party reads by its last six characters, as the graph names it (board L8, R16); the whole
    // lock hash (or address) stays its title and link
    expect(within(table).getByText('…aaaaaa')).toHaveAttribute('title', `0x${'aa'.repeat(32)}`);
    expect(within(table).queryByText('0xaaaaaaaa…aaaaaa')).toBeNull();
    // R15: table rows 32–34 px — rows at body (as the DAO page's depositor rows), a 20.8 px line
    // with 6 px above and below
    for (const cell of within(table).getAllByRole('cell'))
      expect(cell).toHaveClass('text-body', 'py-1.5');
    // every head and cell holds its words in an inline element (R16: a text box is its words)
    for (const cell of [
      ...within(table).getAllByRole('columnheader'),
      ...within(table).getAllByRole('cell'),
    ])
      for (const node of Array.from(cell.childNodes))
        expect(node.nodeType === Node.TEXT_NODE && node.textContent!.trim() !== '').toBe(false);
    // Table-rows T5: the channel id is the row's one bright thing (it read in the id blue).
    const id = within(table).getAllByRole('link')[0];
    expect(id).toHaveAttribute('href', expect.stringMatching(/^\/mainnet\/fiber\/channels\/0x/));
    expect(id).toHaveClass('text-text-bright');
    expect(id).not.toHaveClass('text-[#6aa9c4]');
    expect(Array.from(table.querySelectorAll('tbody .text-text-bright'))).toEqual([id]);
    expect(sorts.every((sort) => sort === 'capacity')).toBe(true);
    expect(screen.getByText('sorted by capacity')).toHaveClass('text-meta');
    expect(screen.getByText('Showing 1–1 of 38 channels · 50 per page')).toBeInTheDocument();
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
    // R11: the state word stays neutral; only its dot keeps Fiber's jade
    const state = within(row).getByText('open');
    expect(state.className).not.toMatch(/(^|\s)text-jade(\s|$)/);
    expect(state.querySelector('.bg-jade')).not.toBeNull();
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
