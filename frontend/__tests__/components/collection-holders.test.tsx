import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  act,
  createTestQueryClient,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
import { CollectionHolders } from '@/components/asset/collection-holders';
import type { CollectionHolder, CursorPaginatedResponse } from '@/lib/api';
import { resourceScale } from '@/components/blocks/block-visual';
import { CLASS_COLORS } from '@/lib/design-tokens';

const barWidth = (value: number, scale: number, track: number) =>
  `${Math.max(Math.round((value / scale) * track * 100) / 100, value > 0 ? 2 : 0)}px`;

/** Every column head is its words in an inline box, never a bare text node filling the cell. */
function expectWordHeads(table: HTMLElement) {
  for (const th of table.querySelectorAll('thead th')) {
    expect(Array.from(th.childNodes).some((n) => n.nodeType === Node.TEXT_NODE)).toBe(false);
    expect(th.firstElementChild?.tagName).toBe('SPAN');
  }
}

const lock = '0x' + '12'.repeat(32);
const prefix = '0x' + '34'.repeat(20);
const page = (
  data: CollectionHolder[],
  extra: Partial<CursorPaginatedResponse<CollectionHolder>> = {}
): CursorPaginatedResponse<CollectionHolder> => ({
  data,
  limit: 10,
  total: 2,
  hasMore: false,
  nextCursor: null,
  ...extra,
});
// Round 3 (contracts R-A · R-2): no indexed lock script → no address and no code hash either.
const named = {
  lockScriptHash: null,
  address: null,
  ownerHashPrefix: prefix,
  itemCount: 6,
  lockCodeHash: null,
  lockKind: null,
};
const resolved = {
  lockScriptHash: lock,
  address: null,
  itemCount: 2,
  lockCodeHash: null,
  lockKind: null,
};
// Unmodified holderDistribution from the real collection-insights producer.
const distribution = [
  { holders: 0, items: 0, range: '1' },
  { holders: 1, items: 2, range: '2' },
  { holders: 0, items: 0, range: '3–5' },
  { holders: 1, items: 6, range: '6+' },
];

describe('Collection holders', () => {
  it('keeps whole-scope distribution and named-only prefixes without inventing addresses', async () => {
    const load = vi.fn().mockResolvedValue(page([named, resolved]));
    render(<CollectionHolders load={load} queryKey="cluster" distribution={distribution} />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(load).toHaveBeenCalledWith({ limit: 10, cursor: undefined });
    const rows = within(table).getAllByRole('row');
    expect(rows[1]).toHaveTextContent('unresolved');
    expect(within(rows[1]).queryByRole('link')).not.toBeInTheDocument();
    expect(within(rows[1]).getByTitle(prefix)).toBeInTheDocument();
    expect(within(rows[2]).getByRole('link')).toHaveAttribute('href', `/mainnet/address/${lock}`);
    expect(screen.getByText('Full collection: 2 holders · 8 items')).toBeInTheDocument();
    expect(within(table).queryByText(/%/)).not.toBeInTheDocument();
    const buckets = screen.getByRole('table', { name: 'Holders by how many items they hold' });
    expect(within(buckets).getAllByRole('row')).toHaveLength(5);
  });

  it('names each holder lock by the registry, else by its code hash (R-2, OB-C7)', async () => {
    const load = vi.fn().mockResolvedValue(
      page([
        { ...resolved, lockCodeHash: `0x${'9b'.repeat(32)}`, lockKind: 'JoyID' },
        {
          ...resolved,
          lockScriptHash: `0x${'56'.repeat(32)}`,
          lockCodeHash: `0x${'43'.repeat(32)}`,
          lockKind: null,
        },
        named,
      ])
    );
    render(<CollectionHolders load={load} queryKey="kinds" />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    const rows = within(table).getAllByRole('row');
    expect(rows[1]).toHaveTextContent('JoyID');
    expect(rows[2]).toHaveTextContent('0x434343...4343');
    // No indexed lock: neither a kind nor a code hash is shown.
    expect(rows[3]).not.toHaveTextContent(/JoyID|0x4343/);
  });

  it('uses backend ordering and cursor pages, and resets mode and cursor for a new collection', async () => {
    const load = vi
      .fn()
      .mockImplementation(({ limit, cursor }) =>
        Promise.resolve(
          cursor
            ? page([resolved], { limit, total: 51 })
            : page([named], { limit, total: 51, hasMore: true, nextCursor: 'next-50' })
        )
      );
    const { rerender } = render(<CollectionHolders load={load} queryKey="first" />);
    await screen.findByRole('table', { name: 'Top 10 collection holders' });
    fireEvent.click(screen.getByRole('button', { name: 'All holders →' }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ limit: 20, cursor: undefined }));
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ limit: 20, cursor: 'next-50' }));
    const table = await screen.findByRole('table', { name: 'All collection holders' });
    expect(within(table).getAllByRole('row')[1]).toHaveTextContent('21');
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled());
    rerender(<CollectionHolders load={load} queryKey="second" />);
    await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(load).toHaveBeenLastCalledWith({ limit: 10, cursor: undefined });
    expect(screen.queryByRole('button', { name: 'Previous' })).not.toBeInTheDocument();
  });

  it('shows refresh failures while retaining the previously read holder rows', async () => {
    const client = createTestQueryClient();
    const load = vi.fn().mockResolvedValue(page([resolved]));
    render(
      <QueryClientProvider client={client}>
        <CollectionHolders load={load} queryKey="retained" />
      </QueryClientProvider>
    );
    await screen.findByRole('table', { name: 'Top 10 collection holders' });
    load.mockRejectedValue(new Error('secondary read failed'));
    await act(async () => {
      await client.refetchQueries({ queryKey: ['collection-holders', 'retained'] });
    });
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not refresh collection holders: secondary read failed. Showing previously loaded data.'
    );
    expect(screen.getByRole('table', { name: 'Top 10 collection holders' })).toBeInTheDocument();
  });

  it('shows a read failure separately from an empty collection', async () => {
    const load = vi.fn().mockRejectedValue(new Error('unavailable'));
    render(<CollectionHolders load={load} queryKey="failure" />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not load collection holders');
    expect(screen.queryByText('No current holders.')).not.toBeInTheDocument();
  });

  it('sums full-scope counts exactly beyond a JavaScript safe integer', async () => {
    render(
      <CollectionHolders
        load={vi.fn().mockResolvedValue(page([], { total: 0 }))}
        queryKey="exact-totals"
        distribution={[
          { range: '1', holders: Number.MAX_SAFE_INTEGER, items: Number.MAX_SAFE_INTEGER },
          { range: '2', holders: 2, items: 4 },
        ]}
      />
    );
    expect(
      screen.getByText(
        'Full collection: 9,007,199,254,740,993 holders · 9,007,199,254,740,995 items'
      )
    ).toBeInTheDocument();
    expect(await screen.findByText('No current holders.')).toBeInTheDocument();
  });

  it.each([
    [{ holders: -1, items: 0, range: '1' }],
    [{ holders: 1, items: Number.MAX_SAFE_INTEGER + 1, range: '6+' }],
    [{ holders: 2, items: 1, range: '1' }],
    [{ holders: 0, items: 1, range: '1' }],
    [distribution[0], distribution[0]],
  ])('rejects invalid full-scope counts or repeated buckets %#', (...buckets) => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      expect(() =>
        render(
          <CollectionHolders
            load={vi.fn().mockResolvedValue(page([]))}
            queryKey="bad-distribution"
            distribution={buckets}
          />
        )
      ).toThrow(/collection|Collection/);
    } finally {
      error.mockRestore();
    }
  });

  it('shows both reads when the cached list and the distribution disagree, without crashing (#6)', async () => {
    // The holder list is served from a 30 s cache; the distribution is read fresh. A holder that
    // merged in between is timing, not a broken invariant.
    const load = vi.fn().mockResolvedValue(page([named, resolved]));
    render(
      <CollectionHolders
        load={load}
        queryKey="moving"
        distribution={[{ holders: 1, items: 8, range: '6+' }]}
      />
    );
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(within(table).getAllByRole('row')).toHaveLength(3);
    expect(screen.getByText('Full collection: 1 holders · 8 items')).toBeInTheDocument();
    expect(screen.getByRole('note')).toHaveTextContent(
      'The holder list (2 shown) and the distribution (1 holder) were read at different moments.'
    );
  });

  it('lets an empty distribution with no listed holders read as genuinely no holders', async () => {
    const load = vi.fn().mockResolvedValue(page([], { total: 0 }));
    render(<CollectionHolders load={load} queryKey="none" distribution={[]} />);
    expect(await screen.findByText('Full collection: 0 holders · 0 items')).toBeInTheDocument();
  });
});

// Table-rows round, lane L2-4: the collection holder tables on the row rules — one 41 px line,
// the holder bright, the lock a chip, the item count as an 8 px bar on the table's scale.
describe('Collection holders on the row rules (table-rows L2-4)', () => {
  const joyid = {
    ...resolved,
    itemCount: 6,
    lockKind: 'JoyID',
    lockCodeHash: `0x${'9b'.repeat(32)}`,
  };
  it('draws one-line rows with the holder bright, the lock a chip and the count as a bar', async () => {
    const load = vi.fn().mockResolvedValue(page([joyid, named]));
    render(
      <CollectionHolders
        load={load}
        queryKey="rows"
        distribution={distribution}
        assetClass="objects"
      />
    );
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    expectWordHeads(table);
    const [, first, second] = within(table).getAllByRole('row');
    expect(within(first).getAllByRole('cell')[0]).toHaveClass('whitespace-nowrap');
    // Rank and holder are the row's bright key, as on the token holders table.
    const rankCell = within(first).getAllByRole('cell')[0];
    expect(rankCell).toHaveTextContent(/^1$/);
    expect(rankCell.firstElementChild).toHaveClass('text-text-bright');
    expect(within(first).getByRole('link').parentElement).toHaveClass(
      '[&_a]:text-text-bright',
      '[&_a]:leading-[18px]',
      'flex-nowrap'
    );
    expect(within(first).getByText('JoyID')).toHaveClass('border', 'rounded-[3px]', 'text-meta');
    const scale = resourceScale([6, 6]);
    expect(first.querySelector('[data-track]')).toHaveStyle({ height: '8px' });
    expect(first.querySelector('[data-segment]')).toHaveStyle({
      width: barWidth(6, scale, 48),
      backgroundColor: CLASS_COLORS.objects,
    });
    expect(
      within(table).getByRole('columnheader', { name: 'Items' }).querySelector('[data-scale-ticks]')
    ).not.toBeNull();
    expect(first).toHaveAttribute('title', `#1 · ${lock} · JoyID · 6 items`);
    expect(second).toHaveAttribute(
      'title',
      `#2 · ${prefix} (unresolved) · lock not indexed · 6 items`
    );
    const buckets = screen.getByRole('table', { name: 'Holders by how many items they hold' });
    expectWordHeads(buckets);
    expect(within(buckets).getByText('3–5')).toHaveClass(
      'text-body',
      'text-text-bright',
      'leading-[18px]'
    );
    for (const td of buckets.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    const bucketBar = within(buckets).getAllByRole('row')[2].querySelector('[data-segment]');
    expect(bucketBar).toHaveStyle({ width: barWidth(1, resourceScale([0, 1, 0, 1]), 48) });
  });
  it('keeps a plain count, without a bar, where the page gives no class colour', async () => {
    const load = vi.fn().mockResolvedValue(page([joyid]));
    render(<CollectionHolders load={load} queryKey="plain" />);
    const table = await screen.findByRole('table', { name: 'Top 10 collection holders' });
    expect(table.querySelector('[data-aligned-bar]')).toBeNull();
    expect(within(table).getAllByRole('row')[1]).toHaveTextContent(/6$/);
  });
});

describe('Collection holders phone rows (board L5)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('gives each holder two lines at 358 px: rank inline, holder, lock, count · the bar', async () => {
    atContainerWidth(358);
    const load = vi
      .fn()
      .mockResolvedValue(
        page([named, { ...resolved, lockKind: 'JoyID', lockCodeHash: `0x${'9b'.repeat(32)}` }])
      );
    render(
      <CollectionHolders
        load={load}
        queryKey="phone"
        distribution={distribution}
        assetClass="objects"
      />
    );
    const rows = await screen.findAllByTestId('holder-row-mobile');
    expect(screen.queryByRole('table', { name: 'Top 10 collection holders' })).toBeNull();
    const second = phoneLines(rows[1]);
    expect(second.map((l) => l.key)).toEqual(['holder', 'share']);
    // The rank is inline at the start of the first line, not a line of its own (B#19).
    expect(rows[1].querySelector('[data-line="holder"] [data-role="rank"]')).toHaveTextContent('2');
    expect(second[0].left).toMatch(/^2.*JoyID$/);
    expect(second[0].right).toBe('2 items');
    // Against the largest holder on the page (6 items), as the table's bar.
    expect(rows[1].querySelector('[data-share-bar]')).toHaveAttribute(
      'data-share-bar',
      `${(2 / 6) * 100}%`
    );
    // The three-column distribution (265 px) still fits a phone as a table (R1) …
    expect(
      screen.getByRole('table', { name: 'Holders by how many items they hold' })
    ).toBeInTheDocument();
  });
  it('folds the distribution into one-line rows where even it does not fit', async () => {
    atContainerWidth(240);
    const load = vi.fn().mockResolvedValue(page([named]));
    render(<CollectionHolders load={load} queryKey="narrow" distribution={distribution} />);
    const buckets = await screen.findAllByTestId('holder-bucket-row-mobile');
    expect(buckets).toHaveLength(4);
    expect(phoneLines(buckets[1])).toEqual([
      { key: 'range', tone: 'body', left: 'holds 2 · 2 items', right: '1holder' },
    ]);
  });
});
