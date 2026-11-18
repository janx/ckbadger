import { render, screen, within } from '@/__tests__/utils/test-utils';
import { TokenDistributionView, TokenHolders } from '@/components/asset/token-holders';
import { api } from '@/lib/api';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import type { TokenDistribution } from '@/lib/api';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatShareTick } from '@/components/ui/share-bar';
import { CLASS_COLORS } from '@/lib/design-tokens';

/** A column head's name: its text, or the name over a bar column's scale ticks. */
const headText = (h: HTMLElement) => h.firstChild?.firstChild?.textContent ?? h.textContent;
const barWidth = (value: number, scale: number, track: number) =>
  `${Math.max(Math.round((value / scale) * track * 100) / 100, value > 0 ? 2 : 0)}px`;

/** Every column head is its words in an inline box, never a bare text node filling the cell. */
function expectWordHeads(table: HTMLElement) {
  for (const th of table.querySelectorAll('thead th')) {
    expect(Array.from(th.childNodes).some((n) => n.nodeType === Node.TEXT_NODE)).toBe(false);
    expect(th.firstElementChild?.tagName).toBe('SPAN');
  }
}
const distribution: TokenDistribution = {
  holdersCount: 2,
  totalSupply: '10000000001',
  largestHolderBalance: '10000000000',
  nextNineBalance: '1',
  remainingBalance: '0',
  unit: 'token',
  decimals: 8,
  buckets: [
    { lowerBound: '0', upperBound: '100000000', holdersCount: 1, balance: '1' },
    { lowerBound: '100000000', upperBound: null, holdersCount: 1, balance: '10000000000' },
  ],
};
describe('Token distribution', () => {
  it('uses exact same-response totals and keeps raw bucket boundaries honest', () => {
    const { rerender } = render(<TokenDistributionView distribution={distribution} symbol="T" />);
    // Table-rows L2-4: a bound prints its exact value without trailing zeros ("1", not
    // "1.00000000"), so a range stays on one line.
    expect(screen.getByRole('table', { name: 'Holders by balance' })).toHaveTextContent(
      '1 and above'
    );
    // the review's item I (re-pinned): a share below the whole reads >99.99%, never 100.00%
    expect(screen.getByRole('img')).toHaveAccessibleName(/Largest holder >99\.99%/);
    rerender(
      <TokenDistributionView
        distribution={{ ...distribution, unit: 'raw', decimals: null }}
        symbol="T"
      />
    );
    expect(screen.getByRole('columnheader', { name: 'Balance · raw' })).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Holders by balance' })).toHaveTextContent(
      '100,000,000 (raw)'
    );
  });
  // Readability F3-e: the bands' columns budgeted from what they hold, at 0.6 em a glyph (the range
  // 14 px, the cells 13, ticks 12) and the 16 px gap after every column but the last. The range is
  // the flexible column: "10,000 – under 100,000" whole at its minimum.
  it('budgets every band column from what it holds: 377 px, a table from it (F3-e)', () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      ceil(at('10,000 – under 100,000', 14) + 16),
      // the 48 px track (its widest ticks, "0 750M", are 43.2), the 10 px gap, "99,999" holders
      48 + 10 + ceil(at('99,999', 13)) + 16,
      // "100.00%", wider than the head's word "supply"; the last column
      ceil(Math.max(at('100.00%', 13), at('supply', 12))),
    ];
    expect(budgets).toEqual([201, 121, 55]);
    expect(budgets.reduce((s, w) => s + w, 0)).toBe(377);
    for (const [width, form] of [
      [1232, 'table'],
      [377, 'table'],
      [376, 'rows'],
      [358, 'rows'],
    ] as const) {
      atContainerWidth(width);
      const { container, unmount } = render(
        <TokenDistributionView distribution={distribution} symbol="T" />
      );
      const list = container.querySelector('[data-list-form]')!;
      expect(list).toHaveAttribute('data-list-form', form);
      if (form === 'table') {
        const table = within(list as HTMLElement).getByRole('table');
        const cols = [...table.querySelectorAll('col')].map((c) => (c as HTMLElement).style);
        expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
        expect(cols[0].width).toBe('auto');
      }
      unmount();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  it('keeps each range on one line so a number never breaks mid-digits (IV-13, L2-4)', () => {
    render(<TokenDistributionView distribution={distribution} symbol="T" />);
    const table = screen.getByRole('table', { name: 'Holders by balance' });
    const range = within(table).getByText('Under 1');
    expect(range).toHaveClass('whitespace-nowrap');
    expect(within(table).getByText('1 and above')).toHaveClass('whitespace-nowrap');
  });
  it('draws holders per range as an 8 px token-blue bar on the table’s scale (L2-4)', () => {
    render(
      <TokenDistributionView
        distribution={{
          ...distribution,
          holdersCount: 41,
          buckets: [
            { lowerBound: '0', upperBound: '100000000', holdersCount: 36, balance: '1' },
            { lowerBound: '100000000', upperBound: null, holdersCount: 5, balance: '10000000000' },
          ],
        }}
        symbol="T"
      />
    );
    const table = screen.getByRole('table', { name: 'Holders by balance' });
    expect(within(table).getAllByRole('columnheader').map(headText)).toEqual([
      'Balance, T',
      'Holders',
      'Of supply',
    ]);
    expectWordHeads(table);
    // The range is the bucket row's key: 13 px bright on its 18 px line (T5).
    expect(within(table).getByText('Under 1')).toHaveClass(
      'text-body',
      'text-text-bright',
      'leading-row'
    );
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    const scale = resourceScale([36, 5]);
    const [, under, above] = within(table).getAllByRole('row');
    expect(under.querySelector('[data-track]')).toHaveStyle({ height: '8px' });
    // Re-pinned on purpose (readability F3-e): the track is 48 px, its head's ticks whole over it
    // under the 48 px a bar keeps at least (it was 40)
    expect(under.querySelector('[data-segment]')).toHaveStyle({
      width: barWidth(36, scale, 48),
      backgroundColor: CLASS_COLORS.tokens,
    });
    expect(above.querySelector('[data-segment]')).toHaveStyle({ width: barWidth(5, scale, 48) });
    expect(above).toHaveTextContent('5');
    expect(above).toHaveTextContent('>99.99%');
  });
  it('names a bucket that starts at 0 and has no upper bound "0 and above" (L2-4)', () => {
    // It used to print "Under " with no bound after it.
    render(
      <TokenDistributionView
        distribution={{
          ...distribution,
          holdersCount: 1,
          largestHolderBalance: '10000000001',
          nextNineBalance: '0',
          buckets: [{ lowerBound: '0', upperBound: null, holdersCount: 1, balance: '10000000001' }],
        }}
        symbol="T"
      />
    );
    expect(screen.getByRole('table', { name: 'Holders by balance' })).toHaveTextContent(
      /^Balance, THolders.*0 and above/
    );
  });
  // Readability 2026-11-18, lane B5: from xl the holder list and the balance bands stood side by
  // side 24 px apart on one row grid, so a holder's row read on into a band's ("…tllakf 562,997.00
  // 2.32%" into "Under 1 · 36 · <0.01%"). Between them now: a gutter twice the old one with a rule
  // in the row-rule ink down its middle (32 px, 1 px, 32 px; a column gap in a table is 16 px).
  it('keeps the holders and the balance bands apart: a gutter with a rule between them (B5)', () => {
    render(
      <TokenDistributionView
        distribution={distribution}
        symbol="T"
        holders={<div data-testid="holders" />}
      />
    );
    const grid = screen.getByTestId('holders').parentElement!;
    expect(grid).toHaveClass('xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]', 'xl:gap-x-8');
    const bands = screen.getByRole('table', { name: 'Holders by balance' });
    const column = Array.from(grid.children).find((c) => c.contains(bands)) as HTMLElement;
    // a column of its own around the list (the list measures its own box, without the gutter)
    expect(column).not.toHaveAttribute('data-list-form');
    expect(column).toHaveClass('xl:border-l', 'xl:border-row-rule', 'xl:pl-8');
  });

  // Readability B5: a head stands over what it names; in a column of a bar and its figure, the
  // figure (the name ends where the figure ends, the ticks stay over the track).
  it('stands each bar column’s name over its figure (B5)', async () => {
    render(<TokenDistributionView distribution={distribution} symbol="T" />);
    const table = screen.getByRole('table', { name: 'Holders by balance' });
    const head = within(table).getAllByRole('columnheader')[1];
    const name = within(head).getByText('Holders');
    expect(name).toHaveClass('justify-end');
    const cell = within(table).getAllByRole('row')[1].children[1] as HTMLElement;
    const track = cell.querySelector<HTMLElement>('[data-track]')!;
    const figure = cell.querySelector<HTMLElement>('[data-aligned-bar] > :last-child')!;
    expect(Number.parseFloat(name.style.width)).toBe(
      Number.parseFloat(track.style.width) + 10 + Number.parseFloat(figure.style.width)
    );
  });

  it('rejects missing holders and mismatched aggregate supply', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <TokenDistributionView distribution={{ ...distribution, holdersCount: 3 }} symbol="T" />
      )
    ).toThrow('buckets do not equal totals');
    expect(() =>
      render(
        <TokenDistributionView
          distribution={{ ...distribution, remainingBalance: '1' }}
          symbol="T"
        />
      )
    ).toThrow('concentration does not equal supply');
    error.mockRestore();
  });
});

describe('Token holders (board L5)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const holders = {
    data: [
      {
        lockScriptHash: `0x${'11'.repeat(32)}`,
        address: `ckb1qrgqep8s${'q'.repeat(40)}tllakf`,
        balance: '56299700000000',
        lockCodeHash: `0x${'9b'.repeat(32)}`,
        lockKind: 'JoyID',
      },
      {
        lockScriptHash: `0x${'22'.repeat(32)}`,
        address: null,
        balance: '1000000000000000',
        lockCodeHash: null,
        lockKind: null,
      },
    ],
    total: 2,
    limit: 10,
    hasMore: false,
    nextCursor: null,
    // 562,997 of 24,255,256.49 is 2.32 %.
    totalSupply: '2425525649000000',
    anchor: null,
  };
  const mount = () => {
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue(holders);
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="RGB++" decimals={8} />);
  };

  // Review fb-logic 7 (the lead's ruling): the share head writes "0" and its scale in hundredths
  // of a percent; the widest tick it writes for any page is five glyphs ("0.75%" where the top
  // ten hold under 1 % each), wider than the "100%" the track was measured from: "0" and five
  // glyphs a glyph apart at 12 px is 50.4 px.
  it('measures the share track from the widest tick its head writes, "0.75%": 50.4 px', async () => {
    const ticks = new Set<string>();
    for (let v = 0; v <= 10_000; v++) ticks.add(formatShareTick(resourceScale([v])));
    const widest = Math.max(...[...ticks].map((t) => t.length));
    expect([widest, ticks.has('0.75%')]).toEqual([5, true]);
    const track = (1 + 1 + widest) * 7.2;
    atContainerWidth(1248);
    // 0.72 % and 0.5 % of the supply: the page's scale is 0.75 %
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      ...holders,
      data: [
        { ...holders.data[0], balance: '72000000' },
        { ...holders.data[1], balance: '50000000' },
      ],
      totalSupply: '10000000000',
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="RGB++" decimals={8} />);
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const ticksRow = within(table)
      .getAllByRole('columnheader')[4]
      .querySelector<HTMLElement>('[data-scale-ticks]')!;
    expect(Array.from(ticksRow.children).map((t) => t.textContent)).toEqual(['0', '0.75%']);
    expect(Number.parseFloat(ticksRow.style.width)).toBeCloseTo(track, 10);
  });

  it('draws the five-column table in the token page’s 554 px column at 1280 and the 608 px band', async () => {
    atContainerWidth(554);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    expect(within(table).getAllByRole('columnheader').map(headText)).toEqual([
      '#',
      'Address',
      'Lock',
      'RGB++',
      'Share',
    ]);
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(554);
    const first = within(table).getAllByRole('row')[1];
    expect(first).toHaveTextContent('JoyID');
    expect(first).toHaveTextContent('2.32%');
    // Table-rows L2-4: the share is an 8 px bar on the page's own scale (the largest share here
    // is 41.23 %), not a 10 %-is-full hairline.
    const scale = resourceScale([232, 4123]);
    expect(first.querySelector('[data-segment]')).toHaveStyle({
      width: barWidth(232, scale, 48),
      backgroundColor: CLASS_COLORS.tokens,
    });
    const second = within(table).getAllByRole('row')[2];
    expect(second.querySelector('[data-segment]')).toHaveStyle({
      width: barWidth(4123, scale, 48),
    });
  });

  // Readability F3-e: every column budgeted from what it holds, at 0.6 em a glyph (the address
  // 14 px, the cells 13, tags and ticks 12) and the 16 px gap after every column but the last.
  // Lock is the flexible column (a name that ends in the ellipsis): its tag holds 12 characters at
  // its minimum. The table still holds in the token page's 554 px column at 1280.
  it('budgets every holder column from what it holds: 552 px, a table from it (F3-e)', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5;
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // a rank of five digits (a token's holders under 100,000), as the rank is written
      ceil(at('99999', 13) + 16),
      // a holder with no address, its lock hash at the row size (an address is 13 characters)
      ceil(at('0x11111111…111111', 14) + 16),
      // the lock's tag: 12 characters at 12 px, 5 px sides and a 1 px border
      ceil(at('x'.repeat(12), 12) + 12 + 16),
      // 12 characters of the balance; a longer one ends in the ellipsis, exact in its title
      ceil(at('x'.repeat(12), 13) + 16),
      // the 48 px track (its ticks "0 100%" are 43.2), the 10 px gap, "100.00%"; the last column
      48 + 10 + ceil(at('100.00%', 13)),
    ];
    expect(budgets).toEqual([55, 159, 115, 110, 113]);
    expect(budgets.reduce((s, w) => s + w, 0)).toBe(552);
    atContainerWidth(554);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const cols = [...table.querySelectorAll('col')].map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[2].width).toBe('auto');
    expect(Number.parseFloat(table.style.minWidth)).toBe(552);
  });

  // Readability B5: the Share head stands over the share, not over its bar.
  it('stands the Share head over the share, the ticks over the track (B5)', async () => {
    atContainerWidth(1248);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const name = within(within(table).getAllByRole('columnheader')[4]).getByText('Share');
    expect(name).toHaveClass('justify-end');
    const cell = within(table).getAllByRole('row')[1].children[4] as HTMLElement;
    const track = cell.querySelector<HTMLElement>('[data-track]')!;
    const figure = cell.querySelector<HTMLElement>('[data-aligned-bar] > :last-child')!;
    expect(figure).toHaveTextContent('2.32%');
    expect(Number.parseFloat(name.style.width)).toBe(
      Number.parseFloat(track.style.width) + 10 + Number.parseFloat(figure.style.width)
    );
  });

  it('writes each holder on one row-density line: rank and address bright, 2-dp amount, the rest in the title (L2-4)', async () => {
    atContainerWidth(1248);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    expectWordHeads(table);
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    const [, first, second] = within(table).getAllByRole('row');
    expect(within(first).getByText('1')).toHaveClass('text-text-bright');
    expect(within(first).getByRole('link')).toHaveClass('text-body', 'text-text-bright');
    // Re-pinned with U-15 (lane A4b): the chip is the lock kind drawn as a row tag, as on the
    // collection holders — the row tag's classes, its hover the lock's code hash (the kind whole
    // in the row's hover) — and a lock not indexed is said so in the row's hover, not "lock kind
    // unavailable".
    expect(within(first).getByText('JoyID')).toHaveAttribute('data-lock-kind');
    expect(within(first).getByText('JoyID')).toHaveClass('text-meta', 'border');
    expect(within(first).getByText('JoyID')).toHaveClass('truncate');
    expect(within(first).getByText('JoyID')).toHaveAttribute('title', `0x${'9b'.repeat(32)}`);
    expect(within(first).getAllByRole('cell')[0]).toHaveClass('whitespace-nowrap');
    // the list's summary form: whole from 1,000 up (readability R5, re-pinned on purpose: was
    // "562,997.00", two places)
    expect(first).toHaveTextContent('JoyID562,9972.32%');
    expect(first).not.toHaveTextContent('562,997.00');
    expect(first).toHaveAttribute(
      'title',
      `#1 · ckb1qrgqep8s${'q'.repeat(40)}tllakf · JoyID · 562,997 RGB++ · 2.32% of supply`
    );
    expect(second).toHaveAttribute(
      'title',
      `#2 · 0x${'22'.repeat(32)} · lock not indexed · 10,000,000 RGB++ · 41.23% of supply`
    );
  });

  it('gives each holder two lines at 358 px: rank inline with address, lock and amount · the bar', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    mount();
    const [first] = await screen.findAllByTestId('holder-row-mobile');
    expect(screen.queryByRole('table', { name: 'Top 10 holders' })).toBeNull();
    expect(phoneLines(first)).toEqual([
      {
        key: 'holder',
        tone: 'head',
        // U-09 (re-pinned): the phone row's address at the tight size
        left: '1ckb1…tllakf JoyID',
        // a phone row's summary form (readability R5, re-pinned on purpose: was "562,997.00000000")
        right: '562,997',
      },
      // Re-pinned with ruling R7 (UI review 2026-11-14): the first row carries the page's axis
      // mark (41.23 % at most: 60 %, the table's), and the share's number sits right after its
      // fixed track.
      { key: 'axis', tone: 'quiet', left: '060%', right: null },
      { key: 'share', tone: 'quiet', left: '2.32%', right: null },
    ]);
    expect(first.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '3.87%');
    expect(first.querySelector('[data-role="rank"]')).toHaveClass('w-[22px]');
  });
});

// U-15 / lane A4b (UI review 2026-11-14): a holder whose lock the scripts registry does not name
// read "Lock kind unavailable", an internal state; the answer now carries the lock's code hash and
// the row names the lock by it, "lock 0x43434343…434343", linked to that code hash's script page,
// as the collection holders do. A registered lock reads as before; a lock not indexed shows no kind.
// U-09 (re-pinned): the code hash takes the size of the form it is drawn in, row in the table and
// tight in the phone row.
describe('Token holders name an unregistered lock by its code hash (U-15)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const unregistered = `0x${'43'.repeat(32)}`;
  const scriptPage = expect.stringMatching(new RegExp(`/script/${unregistered}$`));
  const page = {
    data: [
      {
        lockScriptHash: `0x${'11'.repeat(32)}`,
        address: `ckb1qrgqep8s${'q'.repeat(40)}tllakf`,
        balance: '300',
        lockCodeHash: `0x${'9b'.repeat(32)}`,
        lockKind: 'JoyID',
      },
      {
        lockScriptHash: `0x${'22'.repeat(32)}`,
        address: `ckb1qzvenxve${'q'.repeat(40)}ax426h`,
        balance: '200',
        lockCodeHash: unregistered,
        lockKind: null,
      },
      {
        lockScriptHash: `0x${'33'.repeat(32)}`,
        address: null,
        balance: '100',
        lockCodeHash: null,
        lockKind: null,
      },
    ],
    total: 3,
    limit: 10,
    hasMore: false,
    nextCursor: null,
    totalSupply: '600',
    anchor: null,
  };
  const mount = () => {
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue(page);
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
  };

  it('writes the table’s lock chip "lock <code hash>", the hash a link to its script page', async () => {
    atContainerWidth(1248);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const [, named, unnamed, notIndexed] = within(table).getAllByRole('row');
    expect(within(named).getByText('JoyID')).toHaveClass('truncate');
    expect(named).toHaveAttribute('title', expect.stringContaining(' · JoyID · '));
    expect(unnamed.querySelector('[data-lock-kind]')).toHaveTextContent(/^lock 0x43434343…434343$/);
    expect(within(unnamed).getByRole('link', { name: '0x43434343…434343' })).toHaveAttribute(
      'href',
      scriptPage
    );
    expect(unnamed).toHaveAttribute('title', expect.stringContaining(` · ${unregistered} · `));
    expect(notIndexed.querySelector('[data-lock-kind]')).toBeNull();
    expect(notIndexed).toHaveAttribute('title', expect.stringContaining(' · lock not indexed · '));
    for (const row of [named, unnamed, notIndexed]) {
      expect(row).not.toHaveTextContent(/lock kind unavailable/i);
      expect(row.getAttribute('title')).not.toMatch(/lock kind unavailable/i);
    }
  });

  it('writes the phone row’s lock "lock <code hash>" beside the holder', async () => {
    atContainerWidth(358);
    atViewportWidth(390);
    mount();
    const [named, unnamed, notIndexed] = await screen.findAllByTestId('holder-row-mobile');
    expect(phoneLines(named)[0].left).toMatch(/ JoyID$/);
    expect(phoneLines(unnamed)[0].left).toMatch(/ lock 0x4343…4343$/);
    expect(within(unnamed).getByRole('link', { name: '0x4343…4343' })).toHaveAttribute(
      'href',
      scriptPage
    );
    expect(notIndexed.querySelector('[data-lock-kind]')).toBeNull();
    for (const row of [named, unnamed, notIndexed])
      expect(row).not.toHaveTextContent(/lock kind unavailable/i);
  });
});

// The review's item I: a holder's share is the one percentage, never a false 0.00% or 100.00%
describe('Token holders’ shares at the extremes', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('writes <0.01% for a holder above nothing and >99.99% for one below the whole', async () => {
    atContainerWidth(1248);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          address: `ckb1qrgqep8s${'q'.repeat(40)}tllakf`,
          balance: '999999',
          lockCodeHash: `0x${'9b'.repeat(32)}`,
          lockKind: 'JoyID',
        },
        {
          lockScriptHash: `0x${'22'.repeat(32)}`,
          address: `ckb1qzvenxve${'q'.repeat(40)}ax426h`,
          balance: '1',
          lockCodeHash: `0x${'9b'.repeat(32)}`,
          lockKind: 'JoyID',
        },
      ],
      total: 2,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '1000000',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const [, whale, dust] = within(table).getAllByRole('row');
    expect(whale).toHaveTextContent('>99.99%');
    expect(whale).not.toHaveTextContent('100.00%');
    expect(dust).toHaveTextContent('<0.01%');
    expect(dust).not.toHaveTextContent('0.00%');
  });
});

// A4b review: a holder whose lock has no address form (args longer than an address can carry;
// lock args are unbounded on chain) comes with a null address and its lock's code hash and kind.
// The row names it by its lock hash, linked to the address page that takes a lock hash, and by its
// lock kind — never "lock not indexed", which is said only when there is no code hash.
describe('Token holders name a holder whose lock has no address form by its lock (A4b review)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const lockHash = `0x${'88'.repeat(32)}`;
  const mount = () => {
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: lockHash,
          address: null,
          balance: '100',
          lockCodeHash: `0x${'9b'.repeat(32)}`,
          lockKind: 'SECP256K1/blake160',
        },
      ],
      total: 1,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '100',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
  };
  const addressPage = expect.stringMatching(new RegExp(`/address/${lockHash}$`));

  it('in the table', async () => {
    atContainerWidth(1248);
    mount();
    const table = await screen.findByRole('table', { name: 'Top 10 holders' });
    const [, row] = within(table).getAllByRole('row');
    expect(within(row).getAllByRole('link')[0]).toHaveAttribute('href', addressPage);
    expect(row.querySelector('[data-lock-kind]')).toHaveTextContent('SECP256K1/blake160');
    expect(row).toHaveAttribute(
      'title',
      expect.stringContaining(`${lockHash} · SECP256K1/blake160`)
    );
    expect(row.getAttribute('title')).not.toMatch(/not indexed/);
  });

  it('in the phone row', async () => {
    atContainerWidth(358);
    mount();
    const [row] = await screen.findAllByTestId('holder-row-mobile');
    expect(within(row).getAllByRole('link')[0]).toHaveAttribute('href', addressPage);
    expect(phoneLines(row)[0].left).toMatch(/ SECP256K1\/blake160$/);
    expect(row).not.toHaveTextContent(/not indexed/);
  });
});

describe('Token holders where the table does not fit (L5)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('falls back to the two-line row just below the 548 px table', async () => {
    atContainerWidth(546);
    vi.spyOn(api, 'getTokenHolders').mockResolvedValue({
      data: [
        {
          lockScriptHash: `0x${'11'.repeat(32)}`,
          address: null,
          balance: '100',
          lockCodeHash: `0x${'9b'.repeat(32)}`,
          lockKind: 'JoyID',
        },
      ],
      total: 1,
      limit: 10,
      hasMore: false,
      nextCursor: null,
      totalSupply: '1000',
      anchor: null,
    });
    vi.spyOn(api, 'getTokenDistribution').mockRejectedValue(new Error('not needed'));
    render(<TokenHolders typeHash="0xabc" symbol="T" decimals={0} />);
    const [row] = await screen.findAllByTestId('holder-row-mobile');
    expect(screen.queryByRole('table', { name: 'Top 10 holders' })).toBeNull();
    // Re-pinned with ruling R7: 10.00 % is drawn on the page's rounded maximum (10 %), the axis
    // mark over the bar, the number beside it — no longer full because 10 % was the clamp.
    // R9 (re-pinned): a container just under the table is a desktop column (the holders beside the
    // balance bands from 1280, 545 px), so at jsdom's 1024 px viewport the holder takes the row
    // size, 10…6 for a lock hash (it was the tight 6…4)
    expect(phoneLines(row)).toEqual([
      { key: 'holder', tone: 'head', left: '10x11111111…111111 JoyID', right: '100' },
      { key: 'axis', tone: 'quiet', left: '010%', right: null },
      { key: 'share', tone: 'quiet', left: '10.00%', right: null },
    ]);
    expect(row.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '100%');
  });
});

describe('Holders by balance phone row', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('folds a range into two lines where its three columns do not fit', () => {
    atContainerWidth(300);
    render(<TokenDistributionView distribution={distribution} symbol="T" />);
    const rows = screen.getAllByTestId('holder-bucket-row-mobile');
    expect(phoneLines(rows[1]).map((l) => [l.key, l.left, l.right])).toEqual([
      ['range', '1 and above', '1 holder'],
      ['share', '', '>99.99% of supply'],
    ]);
  });
});
