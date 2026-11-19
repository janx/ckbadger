import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import {
  AssetFeed,
  AssetTransactions,
  itemFeedHeadline,
  tokenMoved,
} from '@/components/asset/asset-feed';
import type { Transaction, GlobalActivity, ItemActivity } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';
// re-pinned (lane c2-assets): the feed's tab row is the controls library's Tabs, a command line
// whose names are lowercase ('activities', 'transactions'; were 'Activities', 'Transactions'), and
// its pager is the board's, a list in time ('older →', '← newer'; were 'Next', 'Previous')
const token = '0x' + 'ab'.repeat(32);
const activity: GlobalActivity = {
  txHash: '0x' + 'cd'.repeat(32),
  blockNumber: 12,
  txIndex: 3,
  timestamp: '1700000000000',
  isCellbase: false,
  participants: [
    {
      lockHash: '0x' + '01'.repeat(32),
      address: null,
      roles: [],
      lockHashPrefix: null,
      tags: 1,
      ckbDelta: '-100000001',
      usedDelta: '0',
      itemDeltas: [{ kind: 'token', typeScriptHash: token, delta: '-13' }],
    },
    {
      lockHash: '0x' + '02'.repeat(32),
      address: null,
      roles: [],
      lockHashPrefix: null,
      tags: 1,
      ckbDelta: '100000000',
      usedDelta: '0',
      itemDeltas: [
        { kind: 'token', typeScriptHash: token, delta: '13' },
        { kind: 'token', typeScriptHash: '0x' + 'ef'.repeat(32), delta: '77' },
      ],
    },
  ],
  typeCalls: [],
  lockCalls: [],
  protocolActions: [],
};
const transaction: Transaction = {
  hash: activity.txHash,
  blockNumber: 12,
  blockHash: '0x' + 'ef'.repeat(32),
  index: 3,
  inputsCount: 2,
  outputsCount: 2,
  fee: '1',
  value: '100000000',
  outputCapacity: '20000000001',
  txSize: 456,
  cycles: null,
  cyclesStatus: 'pending',
  isCellbase: false,
  timestamp: activity.timestamp,
  activity,
};
describe('Shared asset feeds', () => {
  const aptId = '0x' + 'a7'.repeat(20);
  /** One item-feed row; the API states what the tx meant for this item in its itemContext. */
  const row = (context: Partial<ItemActivity['itemContext']>, extra: Partial<ItemActivity> = {}) =>
    ({
      ...activity,
      participants: [{ ...activity.participants[0], itemDeltas: [] }],
      ...extra,
      itemContext: {
        itemId: aptId,
        action: 'update',
        protocolActions: [],
        ringRelink: null,
        actedOn: context.ringRelink ? 'neighbour' : 'self',
        createdCell: null,
        consumedCell: null,
        ...context,
      },
    }) as ItemActivity;
  const cell = { kind: 'identity', protocolName: '.cell' } as const;

  it('names a .cell row by the protocol action the API attributes to this item (ID-C1, #5)', () => {
    expect(itemFeedHeadline(row({ protocolActions: ['edit_records'] }), cell)).toEqual({
      text: '.cell · edit records',
      neighbour: false,
    });
    // A batch tx: another name's action comes first in protocolActions; it is not this row's.
    const batch = row(
      { protocolActions: ['transfer'], action: 'transfer' },
      {
        protocolActions: [
          { protocol: 'dotcell', protocolName: '.cell', action: 'edit_records', metadata: {} },
          { protocol: 'dotcell', protocolName: '.cell', action: 'transfer', metadata: {} },
        ],
      }
    );
    expect(itemFeedHeadline(batch, cell).text).toBe('.cell · transfer');
    // Several own actions in one tx, in order.
    expect(itemFeedHeadline(row({ protocolActions: ['buy', 'list'] }), cell)).toEqual({
      text: '.cell · buy, list',
      neighbour: false,
    });
    // A rewrite nothing else attributes is its own ring_relink action: named, and dimmed exactly
    // when the API says a neighbour acted (contracts §5.1 actedOn), never by a frontend rule.
    expect(
      itemFeedHeadline(row({ protocolActions: ['ring_relink'], actedOn: 'neighbour' }), cell)
    ).toEqual({ text: '.cell · ring pointer changed', neighbour: true });
    expect(itemFeedHeadline(row({ protocolActions: ['ring_relink'] }), cell)).toEqual({
      text: '.cell · ring pointer changed',
      neighbour: false,
    });
  });

  it('names other standards by the exact item action, never a net-delta guess or "Item"', () => {
    // One casing site-wide (OB-C14): sentence case, as the collection rows.
    expect(itemFeedHeadline(row({ action: 'renew' }), { kind: 'identity' }).text).toBe(
      'Identity renew'
    );
    expect(itemFeedHeadline(row({ action: 'transfer' }), { kind: 'object' }).text).toBe(
      'Object transfer'
    );
    expect(() => itemFeedHeadline(row({ action: '' }), { kind: 'identity' })).toThrow(
      'item feed row has no item action'
    );
  });

  it.each([
    ['register', 'Neighbour registered'],
    ['register_subname', 'Neighbour registered a sub-name'],
    ['recycle', 'Neighbour recycled'],
    ['unattributed', 'Ring pointer changed'],
  ] as const)('dims a ring rewrite by a neighbour %s (#1, #5)', (by, text) => {
    const relinked = row({
      ringRelink: { by, byId: by === 'unattributed' ? null : '0x' + 'cc'.repeat(20) },
    });
    expect(itemFeedHeadline(relinked, cell)).toEqual({ text, neighbour: true });
  });

  // F2 identities (round 2), ID-C13: an in-place edit moves no item, so the classifier would
  // leave it unmarked; an item feed's rows all belong to the item and carry its class mark.
  it('marks every identity item-feed row with the identity class, even an in-place edit', async () => {
    const edit = row({ action: 'update', protocolActions: ['edit_records'] });
    render(
      <AssetFeed
        queryKey="identity-mark"
        scope="This identity's whole life"
        item={cell}
        loadActivities={async () =>
          pagedRead({
            data: [edit],
            total: 1,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          })
        }
        loadTransactions={async () =>
          pagedRead({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          })
        }
      />
    );
    const headline = await screen.findByText('.cell · edit records');
    expect(
      headline.closest('[data-role="headline"]')!.querySelector('[aria-label="identities class"]')
    ).not.toBeNull();
  });

  it('keeps the identity page up when a row breaks the feed contract, naming the tx', async () => {
    // T3 shape: shop.v3-first-name registered as a sub-name, rewriting its parent's ring pointer.
    const t3 = row(
      { action: 'update', ringRelink: { by: 'register_subname', byId: '0x' + 'cc'.repeat(20) } },
      {
        protocolActions: [
          {
            protocol: 'dotcell',
            protocolName: '.cell',
            action: 'register_subname',
            metadata: { relinked: aptId, parentId: aptId },
          },
        ],
      }
    );
    const broken = {
      ...row({ ringRelink: { by: 'migrate' as never, byId: null } }),
      txHash: '0x' + 'ee'.repeat(32),
    };
    render(
      <AssetFeed
        queryKey="dotcell:v3-first-name"
        scope="This identity's whole life"
        item={cell}
        loadActivities={async () =>
          pagedRead({
            data: [t3, broken],
            limit: 20,
            hasMore: false,
            nextCursor: null,
          })
        }
        loadTransactions={async () =>
          pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null })
        }
      />
    );
    const headline = await screen.findByText('Neighbour registered a sub-name');
    // R4 (re-pinned on purpose): a neighbour's row is the quiet variant, not 45 % opacity
    expect(headline.closest('article')).toHaveClass('quiet');
    expect(headline.closest('article')).not.toHaveClass('opacity-[0.45]');
    expect(screen.getByTestId('activity-read-error')).toHaveTextContent(
      `Cannot read tx ${broken.txHash}`
    );
  });

  it('names .bit rows in the board words, keeps unknown actions raw in mono, dims neighbours (ID-C1/C4)', async () => {
    const valar = '0x2d2ed79a32f078c511108765a384a183e14990e9';
    const own = row(
      {
        itemId: valar,
        protocolActions: ['dotbit:renew_account', 'dotbit:lock_account_for_cross_chain'],
      },
      { txHash: '0x' + 'a1'.repeat(32) }
    );
    const neighbour = row(
      { itemId: valar, actedOn: 'neighbour' },
      {
        txHash: '0x' + 'a2'.repeat(32),
        protocolActions: [
          {
            protocol: 'dotbit',
            protocolName: '.bit',
            action: 'recycle_expired_account',
            metadata: { accountId: '0x' + 'da'.repeat(20) },
          },
        ],
      }
    );
    render(
      <AssetFeed
        queryKey="dotbit:valar"
        scope="This identity's whole life"
        item={{ kind: 'identity', protocol: 'dotbit' }}
        loadActivities={async () =>
          pagedRead({
            data: [own, neighbour],
            limit: 20,
            hasMore: false,
            nextCursor: null,
          })
        }
        loadTransactions={async () =>
          pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null })
        }
      />
    );
    const renewed = await screen.findByText(/^Renewed/);
    expect(renewed.closest('article')).not.toHaveClass('quiet');
    const raw = renewed.querySelector('code[data-raw-action]')!;
    expect(raw).toHaveTextContent('dotbit:lock_account_for_cross_chain');
    expect(raw).toHaveClass('font-mono');
    expect(screen.getByText('Neighbour recycled').closest('article')).toHaveClass('quiet');
  });

  it('counts this token once from positive participant deltas, not CKB value or both sides', () => {
    expect(tokenMoved(transaction, token)).toBe('13');
    expect(() => tokenMoved({ ...transaction, activity: null }, token)).toThrow(
      'missing its full activity'
    );
  });
  it('renders exact amounts and explicit pending cycles in the transaction layer', () => {
    render(
      <AssetTransactions
        rows={[transaction]}
        token={{ typeHash: token, symbol: 'T', decimals: 8 }}
      />
    );
    const table = screen.getByRole('table', { name: 'Asset transactions' });
    expect(table).toHaveTextContent('0.00000013');
    expect(table).toHaveTextContent('200.00000001');
    expect(table).toHaveTextContent('Not computed');
    expect(table).toHaveTextContent('tx 3 in block');
  });
  it('keeps both layers with independent cursors and exact tx identity', async () => {
    const loadActivities = vi
      .fn()
      .mockResolvedValue(
        pagedRead({ data: [activity], limit: 1, hasMore: true, nextCursor: 'older' })
      );
    const loadTransactions = vi
      .fn()
      .mockResolvedValue(
        pagedRead({ data: [transaction], limit: 20, hasMore: false, nextCursor: null })
      );
    render(
      <AssetFeed
        queryKey="token:test"
        scope="This token"
        loadActivities={loadActivities}
        loadTransactions={loadTransactions}
      />
    );
    await waitFor(() =>
      expect(loadActivities).toHaveBeenCalledWith({ limit: 20, cursor: undefined })
    );
    fireEvent.click(screen.getByRole('tab', { name: 'transactions' }));
    expect(await screen.findByRole('table', { name: 'Asset transactions' })).toHaveTextContent(
      'Not computed'
    );
    expect(loadTransactions).toHaveBeenCalledWith({ limit: 20, cursor: undefined });
  });
  // The controls lane (C2): the full feed opens on the controls board's tab row as the section's
  // command line (the prompt square, lowercase names; an h2 keeps the section in the outline) and
  // pages as a list in time, the page counted from the newest end and its pages from the count.
  it('opens on the board’s tab row and says where a page of the feed stands', async () => {
    // a full page of 20 of the 100 (lane cd-fixes-controls, SHOULD-5: one row on a page of twenty
    // that said more followed)
    const loadActivities = vi.fn().mockImplementation(async ({ cursor }) =>
      pagedRead({
        data: Array.from({ length: 20 }, (_, i) => ({
          ...activity,
          txHash: `0x${(8192 + i + (cursor ? 20 : 0)).toString(16).padStart(64, '0')}`,
        })),
        total: 100,
        limit: 20,
        hasMore: true,
        nextCursor: cursor ? 'older-still' : 'older',
      })
    );
    const { container } = render(
      <AssetFeed
        queryKey="where"
        scope="This token"
        loadActivities={loadActivities}
        loadTransactions={async () =>
          pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null })
        }
      />
    );
    const feed = screen.getByRole('region', { name: 'What happened' });
    expect(within(feed).getByRole('heading', { level: 2, name: 'What happened' })).toHaveClass(
      'sr-only'
    );
    const row = within(feed).getByRole('tablist', { name: 'What happened' });
    expect(
      within(row)
        .getAllByRole('tab')
        .map((t) => t.textContent)
    ).toEqual(['activities', 'transactions']);
    expect(row.querySelector('.section-mark')).not.toBeNull();
    expect(feed).toHaveTextContent('This token · newest first');
    const pager = await screen.findByRole('navigation', { name: 'activities' });
    expect(pager).toHaveTextContent(/^newest first← newerpage 1 of 5older →$/);
    fireEvent.click(within(pager).getByRole('button', { name: 'older →' }));
    await waitFor(() =>
      expect(loadActivities).toHaveBeenLastCalledWith({ limit: 20, cursor: 'older' })
    );
    expect(await screen.findByText('page 2 of 5')).toBeInTheDocument();
    expect(
      within(screen.getByRole('navigation', { name: 'activities' })).getByRole('button', {
        name: '← newer',
      })
    ).toBeInTheDocument();
    expect(container.querySelector('[role="tabpanel"]')).toContainElement(
      screen.getByRole('navigation', { name: 'activities' })
    );
  });
  it('hydrates an activity cursor and returns to that exact starting page without affecting transactions', async () => {
    // re-pinned (lane cd-fixes-controls, item 1): the pages agree with themselves (pagedRead), a
    // page of one activity with an older one after it, then that older and last one (it served
    // the same page at every cursor, whose next cursor repeated the one it was asked with: an API
    // fault the pager now refuses; and 1 row on a page of 20 with more after it)
    const loadActivities = vi.fn(async ({ cursor }: { cursor?: string }) =>
      cursor === 'older'
        ? pagedRead({ data: [activity], total: 2, limit: 1, hasMore: false, nextCursor: null }, 1)
        : pagedRead({ data: [activity], total: 2, limit: 1, hasMore: true, nextCursor: 'older' })
    );
    const loadTransactions = vi
      .fn()
      .mockResolvedValue(
        pagedRead({ data: [], total: 0, limit: 20, hasMore: false, nextCursor: null })
      );
    const changed = vi.fn();
    render(
      <AssetFeed
        queryKey="cursor"
        scope="This identity"
        initialActivityCursor="100:2"
        onActivityCursorChange={changed}
        loadActivities={loadActivities}
        loadTransactions={loadTransactions}
      />
    );
    await waitFor(() =>
      expect(loadActivities).toHaveBeenLastCalledWith({ limit: 20, cursor: '100:2' })
    );
    // a page opened at a cursor from the URL is not counted from the newest end: it names no page
    // (the old pager wrote "Page 1 of 5" there)
    expect(await screen.findByRole('navigation', { name: 'activities' })).not.toHaveTextContent(
      /page/
    );
    fireEvent.click(await screen.findByRole('button', { name: 'older →' }));
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith('older'));
    fireEvent.click(await screen.findByRole('button', { name: '← newer' }));
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith('100:2'));
    fireEvent.click(screen.getByRole('tab', { name: 'transactions' }));
    await waitFor(() =>
      expect(loadTransactions).toHaveBeenCalledWith({ limit: 20, cursor: undefined })
    );
  });

  // The page lane notes (the placement lanes, 2026-11-19): the feed's onNewer was not available on
  // an activity page opened from a URL cursor, so a reader who landed on page 3 could not go
  // newer. The API pages one way (older); the way newer from a cursor it has no history of is the
  // newest end, after which the walk counts its pages again.
  it('goes newer from an activity page opened at a URL cursor: to the newest end', async () => {
    // re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-5): a full page of 20 of
    // the 100 with more after it, through pagedRead (it was 1 row on a page of 20 that said more
    // followed)
    const loadActivities = vi.fn().mockResolvedValue(
      pagedRead({
        data: Array.from({ length: 20 }, (_, i) => ({
          ...activity,
          txHash: `0x${(4096 + i).toString(16).padStart(64, '0')}`,
        })),
        total: 100,
        limit: 20,
        hasMore: true,
        nextCursor: 'older',
      })
    );
    const changed = vi.fn();
    render(
      <AssetFeed
        queryKey="cursor-newer"
        scope="This identity"
        initialActivityCursor="100:2"
        onActivityCursorChange={changed}
        loadActivities={loadActivities}
        loadTransactions={vi.fn()}
      />
    );
    await waitFor(() =>
      expect(loadActivities).toHaveBeenLastCalledWith({ limit: 20, cursor: '100:2' })
    );
    const pager = await screen.findByRole('navigation', { name: 'activities' });
    fireEvent.click(within(pager).getByRole('button', { name: '← newer' }));
    await waitFor(() =>
      expect(loadActivities).toHaveBeenLastCalledWith({ limit: 20, cursor: undefined })
    );
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith(undefined));
    // at the newest end the pages are counted again, and there is nothing newer
    await waitFor(() =>
      expect(screen.getByRole('navigation', { name: 'activities' })).toHaveTextContent(
        'page 1 of 5'
      )
    );
    expect(screen.queryByRole('button', { name: '← newer' })).toBeNull();
  });
});
