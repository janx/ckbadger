import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { atViewportWidth } from '@/__tests__/utils/viewport';
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
    // re-pinned (lane C2 topics): the state is a radio of the controls board's filter chips, in
    // its lowercase words (it was the pressed button "All")
    fireEvent.click(screen.getByRole('radio', { name: /^all/ }));
    await waitFor(() => expect(releaseAll).toBeTypeOf('function'));
    expect(screen.queryByTitle(channel.channelId)).not.toBeInTheDocument();
    // Nor a stale count: while the chosen list loads the heading runs on nothing (phase 5, B7).
    expect(screen.getByRole('heading', { level: 2, name: 'Channels' })).toBeInTheDocument();
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
    // R13: one short lede (≤ 90 characters) and the way down to the explanation in tier 2; in the
    // reading ink, a sentence that explains (re-pinned with readability ruling R3; it was dim)
    const lede = within(hero).getByText(/^Channels touch the chain only to open and to close\./);
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[560px])
    expect(lede).toHaveClass('text-body', 'text-text', 'max-w-measure');
    // one line of the measure with room to spare, as the DAO page's lede: at most 67 characters,
    // where the measure holds MEASURE_CHARS, 76, at the body step (it said 560 px, 8.3 px a
    // character at 13 px)
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
      // the figure's companion in the summary form (readability R5, re-pinned on purpose: was
      // "3,000.00000000 CKB")
    ).toEqual(['3,000 CKB']);
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
      // re-pinned (A4 round 2): the count a unit, number and noun joined by a plain space
      '2 · in 1 pair',
      '1m 40s · 2 of 2 closed within an hour',
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
        // re-pinned (lane C2 topics): the API answers one a page, so its 38 channels take 38
        // pages (it answered 50 a page with a next page, which the old pager drew and the board's
        // refuses: 38 rows at 50 a page are one page)
        return HttpResponse.json({
          ...page([channel]),
          total: 38,
          limit: 1,
          hasMore: true,
          nextCursor: '1:0x01',
        });
      })
    );
    render(<FiberChannelsPage />);
    // Phase 5 (B7): the list as a command, run on its count in the chosen state once counted.
    const heading = await screen.findByRole('heading', { name: 'Channels 38 open' });
    expect(heading).toHaveClass('prompt', 'text-section');
    expect(heading).toHaveAttribute('id', 'fiber-channels-heading');
    expect(heading.lastElementChild).toHaveTextContent(/^38 open$/);
    expect(heading.lastElementChild).toHaveClass('text-text-dim');
    expect(screen.getByRole('region', { name: 'Channels 38 open' })).toContainElement(heading);
    await waitFor(() => expect(requested[0]).toBe('open'));
    // re-pinned (lane C2 topics): the states are the controls board's filter chips, a radio group
    // named "state" in the chips' lowercase words, the summary's count after each (they were
    // pressed buttons, "Open 1" …, in a group named "State")
    const states = screen.getByRole('radiogroup', { name: 'state' });
    const open = await within(states).findByRole('radio', { name: 'open 1' });
    expect(open).toHaveAttribute('aria-checked', 'true');
    for (const name of [
      'closed together 1',
      'force-closed, settled 0',
      'force-closed, not settled 1',
      'all 3',
    ])
      expect(within(states).getByRole('radio', { name })).toHaveAttribute('aria-checked', 'false');
    const table = await screen.findByRole('table', { name: 'Open channels' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Channel', 'Parties', 'Capacity', 'Opened', 'Open for']);
    expect(within(table).getByText('2023-11-15')).toBeInTheDocument();
    expect(within(table).getByText('3m 20s')).toBeInTheDocument();
    // U-09 (re-pinned): a party reads head and tail at the row size (it was its last six
    // characters alone, as the graph named it); the whole lock hash (or address) stays its title
    // and link
    expect(within(table).getByText('0xaaaaaaaa…aaaaaa').closest('[title]')).toHaveAttribute(
      'title',
      `0x${'aa'.repeat(32)}`
    );
    expect(within(table).queryByText('…aaaaaa')).toBeNull();
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
    expect(id).not.toHaveClass('text-[color:var(--ink)]'); // the link cyan (BOARD_INKS.link)
    expect(Array.from(table.querySelectorAll('tbody .text-text-bright'))).toEqual([id]);
    expect(sorts.every((sort) => sort === 'capacity')).toBe(true);
    // The sort stays the right-prompt note, outside the heading's name.
    const sorted = screen.getByText('sorted by capacity');
    expect(sorted).toHaveClass('text-meta');
    expect(sorted).toHaveAttribute('data-prompt-note');
    expect(sorted.previousElementSibling).toBe(heading);
    // re-pinned (lane C2 topics): the controls board's pager for a ranked list, its figures the
    // response's (it was "Showing 1–1 of 38 channels · 50 per page" beside boxed buttons)
    const pager = screen.getByRole('navigation', { name: 'channels' });
    expect(pager).toHaveTextContent('1–1 of 38 · 1 a page');
    expect(pager).toHaveTextContent('← prevpage 1 of 38next →');
    expect(within(pager).queryByRole('button', { name: '← prev' })).toBeNull();
    expect(within(pager).getByRole('button', { name: 'next →' })).toBeInTheDocument();
    fireEvent.click(within(states).getByRole('radio', { name: 'closed together 1' }));
    await waitFor(() => expect(requested).toContain('cooperativelyClosed'));
    expect(
      await screen.findByRole('table', { name: 'Closed together channels' })
    ).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'Lifetime' })).toBeInTheDocument();
    // The args follow the chosen state, its chip's words in the command's lower case.
    expect(
      screen.getByRole('heading', { level: 2, name: 'Channels 38 closed together' })
    ).toBeInTheDocument();
  });

  // Lane C2 (topics), written before the pager and the state chips are replaced: the list pages
  // forward by the cursor the API served and back to the page before it, and a state starts the
  // list again from its first page, so the replacements are proven against what the page did.
  it('pages by the served cursor and back, and a state starts the list again from its first page', async () => {
    const asked: { state: string | null; cursor: string | null }[] = [];
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', ({ request }) => {
        const params = new URL(request.url).searchParams;
        const cursor = params.get('cursor');
        asked.push({ state: params.get('state'), cursor });
        const second = cursor === 'after-first';
        // two channels, one a page: the second page is the last
        return HttpResponse.json({
          data: [second ? closedChannel : channel],
          total: 2,
          limit: 1,
          hasMore: !second,
          nextCursor: second ? null : 'after-first',
        });
      })
    );
    // re-pinned (lane C2 topics): the board's pager's words and the state's radios (they were the
    // buttons Next and Previous, and the pressed button "Closed together 1")
    const next = () => screen.getByRole('button', { name: 'next →' });
    const previous = () => screen.getByRole('button', { name: '← prev' });
    const state = (name: string) => screen.getByRole('radio', { name });
    render(<FiberChannelsPage />);
    expect(await screen.findByTitle(channel.channelId)).toBeInTheDocument();
    expect(asked).toEqual([{ state: 'open', cursor: null }]);
    fireEvent.click(next());
    expect(await screen.findByTitle(closedChannel.channelId)).toBeInTheDocument();
    expect(screen.queryByTitle(channel.channelId)).toBeNull();
    expect(asked.at(-1)).toEqual({ state: 'open', cursor: 'after-first' });
    fireEvent.click(previous());
    expect(await screen.findByTitle(channel.channelId)).toBeInTheDocument();
    expect(screen.queryByTitle(closedChannel.channelId)).toBeNull();
    fireEvent.click(next());
    expect(await screen.findByTitle(closedChannel.channelId)).toBeInTheDocument();
    fireEvent.click(await waitFor(() => state('closed together 1')));
    await waitFor(() =>
      expect(asked.at(-1)).toEqual({ state: 'cooperativelyClosed', cursor: null })
    );
    expect(await screen.findByTitle(channel.channelId)).toBeInTheDocument();
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

  // Readability F3-e: every column budgeted from the widest value it holds, at 0.6 em a glyph (the
  // row's 14 px, a meta line's and the unit's 12) and the 16 px gap after every column but the
  // last. The parties are the flexible column: one party whole at its minimum (a lock hash at the
  // row size, the wider form), the pair on one line where the container gives more.
  it('budgets every column from what it holds and changes form at their sum (F3-e)', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // the channel id at the row size, or the widest state under it ("all" lists the state)
      ceil(Math.max(at('0x01010101…010101', 14), at('force-closed · not settled', 12)) + 16),
      // one party, its lock hash at the row size
      ceil(at('0xbbbbbbbb…bbbbbb', 14) + 16),
      // a capacity under a billion CKB, every decimal, and its unit
      ceil(at('999,999,999.99999999', 14) + at(' CKB', 12) + 16),
      // the date it opened
      ceil(at('2023-11-15', 14) + 16),
      // four digits of days and the hours; the last column
      ceil(at('9999d 23h', 14)),
    ];
    expect(budgets).toEqual([204, 159, 213, 100, 76]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(752);
    serve();
    const { unmount } = render(<FiberChannelsPage />);
    const table = await screen.findByRole('table', { name: 'Open channels' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[1].width).toBe('auto');
    expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
    unmount();
    for (const [width, form] of [
      [1232, 'table'],
      [752, 'table'],
      [751, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      atContainerWidth(width);
      atViewportWidth(width < 608 ? 390 : 1280);
      serve();
      const { unmount: drop } = render(<FiberChannelsPage />);
      const shown = await screen.findByRole(form === 'table' ? 'table' : 'list', {
        name: form === 'table' ? 'Open channels' : 'Open channels, compact',
      });
      expect(shown.closest('[data-list-form]')).toHaveAttribute('data-list-form', form);
      drop();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  it('labels the date and the duration and names the pair by address suffix at 358 px', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    expect(screen.queryByRole('table', { name: 'Open channels' })).toBeNull();
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      // U-09 (re-pinned): the phone row's ids at the tight size
      ['channel', 'head', '0x0101…0101', 'open'],
      [
        'pair',
        'body',
        'ckb1…pr3prn ⇄ 0xbbbb…bbbb Default Lock · unnamed lock',
        // a phone row's summary form (readability R5, re-pinned on purpose: was "3,000.00000000
        // CKB")
        '3,000 CKB',
      ],
      [
        'time',
        'quiet',
        expect.stringMatching(/^opened 2023-11-15 \d\d:\d\d · open for 3m 20s$/),
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
    atViewportWidth(640);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    // R9 (re-pinned): at 640 the row is drawn from sm, beside the network reading that writes the
    // same parties at the row size: the row size (it was the tight size)
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      [
        'channel',
        '0x01010101…010101 · ckb1…qqpr3prn ⇄ 0xbbbbbbbb…bbbbbb Default Lock · unnamed lock',
        // a phone row's summary form (readability R5, re-pinned on purpose: "3,000.00000000 CKB")
        '3,000 CKB',
      ],
      ['time', expect.stringMatching(/^opened 2023-11-15 .+ · open for 3m 20s$/), 'open'],
    ]);
  });
});

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('FiberChannelsPage while its read is paused', () => {
  it('says it is loading, not nothing, then lists the channels once back online', async () => {
    server.use(
      http.get('/api/:network/v1/fiber/network', () => HttpResponse.json(network)),
      http.get('/api/:network/v1/fiber/channels', () => HttpResponse.json(page([channel])))
    );
    const { resume } = renderPaused(<FiberChannelsPage />);
    expect(await screen.findByText('Loading channels…')).toBeInTheDocument();
    resume();
    expect(await screen.findByTitle(channel.channelId)).toBeInTheDocument();
    expect(screen.queryByText('Loading channels…')).toBeNull();
  });
});
