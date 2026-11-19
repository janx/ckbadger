import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderPaused } from '@/__tests__/utils/paused-read';
import {
  atContainerWidth,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { render, screen, waitFor, fireEvent, within } from '../utils/test-utils';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import FiberChannelsPage from '@/app/fiber/channels/client-page';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';
import network from '../fixtures/fiber-network.json';
import channel from '../fixtures/fiber-open.json';
import closedChannel from '../fixtures/fiber-force-closed.json';
import { pagedRead } from '@/__tests__/utils/paged-read';

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const page = (rows: unknown[]) =>
  pagedRead({
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
    // Re-pinned on purpose (lane P4, the link-look gate): the way down is a way onward, its look
    // the link owner's (celadon, its "↓" written by the component and hidden from the name); it was
    // a raw anchor with a typed arrow and a hand-written hover
    const how = within(lede).getByRole('link', { name: 'How it works' });
    expect(how).toHaveAttribute('href', '#evidence');
    expect(how).toHaveClass('text-jade');
    expect(how.textContent).toBe('How it works\u00a0↓');
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
    // R11: the figure is tier 1's only jade text. Re-pinned on purpose (lane P4): but for the way
    // onward, whose celadon is its look's (components/ui/link.tsx), not the page's colour
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    const jade = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => /^text-jade(\/\d+)?$/.test(c))
    );
    expect(jade).toEqual([how, figure]);
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
  });

  // Re-pinned on purpose (lane P4, the plan's P4 sentence: "keys that a tier-1 figure needs come
  // up from tier 2 as a legend, not as prose"; renamed, it said "moves the concept and the graph
  // explainer to tier 2, every sentence kept"): the graph's explainer is its key in tier 1 now,
  // and tier 2 keeps the concept and what a party is, in one column (it was two, the explainer's
  // "How to read the graph" the second)
  it('keeps the concept in tier 2; how to read the graph is its key in tier 1 (P4)', async () => {
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
      await within(tier2).findByText(/Parties are on-chain addresses, not Fiber node ids/)
    ).toBeInTheDocument();
    expect(within(tier2).queryByRole('heading', { name: 'How to read the graph' })).toBeNull();
    expect(tier2).not.toHaveTextContent(/Each circle is a party/);
    const columns = tier2.querySelector(':scope > div')!.children;
    expect(columns).toHaveLength(1);
    expect(columns[0]).toHaveTextContent(/Fiber is a payment-channel network/);
    expect(columns[0]).toHaveTextContent(/Parties are on-chain addresses/);
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    expect(tier1).not.toHaveTextContent(/Fiber is a payment-channel network/);
    expect(tier1).not.toHaveTextContent(/Parties are on-chain addresses/);
    expect(within(tier1).getByRole('list', { name: 'Graph key' })).toHaveTextContent(
      /party · size and number: its channels/
    );
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
        return HttpResponse.json(
          pagedRead({
            ...page([channel]),
            total: 38,
            limit: 1,
            hasMore: true,
            nextCursor: '1:0x01',
          })
        );
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
        return HttpResponse.json(
          pagedRead({
            data: [second ? closedChannel : channel],
            total: 2,
            limit: 1,
            hasMore: !second,
            nextCursor: second ? null : 'after-first',
          })
        );
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
  // last. Re-pinned on purpose (lane P4, review fiber-channels: "every amount .00000000 CKB, some
  // 380 px empty between Parties and Capacity"): the capacity is a list column's figure, the
  // summary form (R5; it was every decimal, 213 px); the parties hold the pair on one line, two
  // lock hashes at the row size and the arrow between them, so the pair never wraps; and the
  // flexible column is the last, the lifetime, read from its left, so the room the container gives
  // beyond the sum stands at the row's end, not between the parties and their capacity (it was
  // the parties', one party at its minimum).
  it('budgets every column from what it holds and changes form at their sum (F3-e)', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // the channel id at the row size, or the widest state under it ("all" lists the state)
      ceil(Math.max(at('0x01010101…010101', 14), at('force-closed · not settled', 12)) + 16),
      // the pair on one line: two lock hashes at the row size, "↔ " before the second, and the
      // 6 px between the two parties
      ceil(at('0xbbbbbbbb…bbbbbb↔ 0xbbbbbbbb…bbbbbb', 14) + 6 + 16),
      // a capacity under a billion CKB in the summary form, and its unit
      ceil(at('999,999,999', 14) + at(' CKB', 12) + 16),
      // the date it opened
      ceil(at('2023-11-15', 14) + 16),
      // four digits of days and the hours; the last column
      ceil(at('9999d 23h', 14)),
    ];
    expect(budgets).toEqual([204, 325, 138, 100, 76]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(843);
    serve();
    const { unmount } = render(<FiberChannelsPage />);
    const table = await screen.findByRole('table', { name: 'Open channels' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols.map((c) => c.width)).toEqual(['204px', '325px', '138px', '100px', 'auto']);
    // the lifetime reads from its left (not a numeric column), so the room stands after it
    const lifetime = within(table).getByText('3m 20s').closest('td')!;
    expect(lifetime).not.toHaveClass('text-right');
    // the capacity in the summary form, its unit after it
    expect(
      Array.from(table.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['3,000 CKB']);
    expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
    unmount();
    for (const [width, form] of [
      [1232, 'table'],
      [843, 'table'],
      [842, 'rows'],
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

  // Re-pinned on purpose (lane D4, PB-17): the phone row states the table's fields — the capacity is
  // line 1's figure (it was the state, now on the quiet line only where the list is of every state),
  // the parties are joined by the table's "↔" with no lock (the table writes none; it was "⇄" and
  // "Default Lock · unnamed lock"), and the opening is its day, named (it was the time to the minute).
  it('labels the date and the duration and names the pair by address suffix at 358 px', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    expect(screen.queryByRole('table', { name: 'Open channels' })).toBeNull();
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      // U-09 (re-pinned): the phone row's ids at the tight size; readability R5, the summary form
      ['channel', 'head', '0x0101…0101', '3,000 CKB'],
      ['parties', 'body', 'ckb1…pr3prn↔ 0xbbbb…bbbb', null],
      ['time', 'quiet', 'opened 2023-11-15 · open for 3m 20s', null],
    ]);
    expect(within(row).getByTitle(channel.channelId)).toHaveAttribute(
      'href',
      `/mainnet/fiber/channels/${channel.channelId}`
    );
  });

  // Re-pinned on purpose (lane D4): the card at 608 px is the phone row's three parts (it was two
  // lines, the id and the pair with the capacity, then the time and the state).
  it('is the phone row’s three parts at 608 px, its ids at the row size from sm', async () => {
    atContainerWidth(608);
    atViewportWidth(640);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    // R9 (re-pinned): at 640 the row is drawn from sm, beside the network reading that writes the
    // same parties at the row size: the row size (it was the tight size)
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['channel', '0x01010101…010101', '3,000 CKB'],
      ['parties', 'ckb1…qqpr3prn↔ 0xbbbbbbbb…bbbbbb', null],
      ['time', 'opened 2023-11-15 · open for 3m 20s', null],
    ]);
  });

  // Lane D4 (the phone-rows board; PB-17): the phone row and the table state the same fields. The
  // table: the channel (and its state where the list is of every state), its parties, its
  // capacity, the day it opened and how long it has been open or lived. The phone row stated a
  // state always, the parties' locks and the opening's time to the minute, none of which the table
  // writes; it is the board's row now: the channel and its capacity, the list's sort; the parties,
  // joined as the table joins them; quiet, the day it opened and how long, named, and its state
  // where the list is of every state.
  it('states the table’s fields at 358 px: channel and capacity · parties · opened and open for (D4)', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    serve();
    render(<FiberChannelsPage />);
    const [row] = await screen.findAllByTestId('fiber-channel-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['channel', 'head', '0x0101…0101', '3,000 CKB'],
      // the parties as the table joins them: each its own item, the arrow before the second
      ['parties', 'body', 'ckb1…pr3prn↔ 0xbbbb…bbbb', null],
      ['time', 'quiet', 'opened 2023-11-15 · open for 3m 20s', null],
    ]);
    // the table writes no lock of a party and no state on a list of one state: nor does the row
    expect(row.textContent).not.toMatch(/Default Lock|unnamed lock|⇄/);
    const labels = Array.from(row.querySelectorAll('[data-role="figure-label"]'));
    expect(labels.map((l) => l.textContent)).toEqual(['opened', 'open for']);
    expect(statedHeadPx(row)).toBe(219);
    expect(phoneHeadPx(row, 360)).toBeLessThanOrEqual(statedHeadPx(row));
  });

  it('adds the state where the list is of every state, as the table does (D4)', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    serve();
    render(<FiberChannelsPage />);
    await screen.findAllByTestId('fiber-channel-row-mobile');
    fireEvent.change(screen.getByRole('combobox', { name: 'state' }), {
      target: { value: 'all' },
    });
    await waitFor(() =>
      expect(phoneLines(screen.getAllByTestId('fiber-channel-row-mobile')[0])[2].left).toBe(
        'opened 2023-11-15 · lifetime 3m 20s · open'
      )
    );
    const [row] = screen.getAllByTestId('fiber-channel-row-mobile');
    // R11: the state word stays neutral; only its dot keeps Fiber's jade
    const state = within(row).getByText('open');
    expect(state.className).not.toMatch(/(^|\s)text-jade(\s|$)/);
    expect(state.querySelector('.bg-jade')).not.toBeNull();
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
