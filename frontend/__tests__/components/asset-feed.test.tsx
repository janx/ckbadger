import { render, screen, fireEvent, waitFor } from '@/__tests__/utils/test-utils';
import {
  AssetFeed,
  AssetTransactions,
  itemFeedHeadline,
  tokenMoved,
} from '@/components/asset/asset-feed';
import type { Transaction, GlobalActivity, ItemActivity } from '@/lib/api';
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
    // A rewrite nothing else attributes is its own ring_relink action: named, dimmed.
    expect(itemFeedHeadline(row({ protocolActions: ['ring_relink'] }), cell)).toEqual({
      text: '.cell · ring pointer changed',
      neighbour: true,
    });
  });

  it('names other standards by the exact item action, never a net-delta guess or "Item"', () => {
    expect(itemFeedHeadline(row({ action: 'renew' }), { kind: 'identity' }).text).toBe(
      'Identity Renew'
    );
    expect(itemFeedHeadline(row({ action: 'transfer' }), { kind: 'object' }).text).toBe(
      'Object Transfer'
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
        loadActivities={async () => ({
          data: [t3, broken],
          limit: 50,
          hasMore: false,
          nextCursor: null,
        })}
        loadTransactions={async () => ({ data: [], limit: 50, hasMore: false, nextCursor: null })}
      />
    );
    const headline = await screen.findByText('Neighbour registered a sub-name');
    expect(headline.closest('article')).toHaveClass('opacity-[0.45]');
    expect(screen.getByTestId('activity-read-error')).toHaveTextContent(
      `Cannot read tx ${broken.txHash}`
    );
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
      .mockResolvedValue({ data: [activity], limit: 50, hasMore: true, nextCursor: 'older' });
    const loadTransactions = vi
      .fn()
      .mockResolvedValue({ data: [transaction], limit: 50, hasMore: false, nextCursor: null });
    render(
      <AssetFeed
        queryKey="token:test"
        scope="This token"
        loadActivities={loadActivities}
        loadTransactions={loadTransactions}
      />
    );
    await waitFor(() =>
      expect(loadActivities).toHaveBeenCalledWith({ limit: 50, cursor: undefined })
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
    expect(await screen.findByRole('table', { name: 'Asset transactions' })).toHaveTextContent(
      'Not computed'
    );
    expect(loadTransactions).toHaveBeenCalledWith({ limit: 50, cursor: undefined });
  });
  it('hydrates an activity cursor and returns to that exact starting page without affecting transactions', async () => {
    const loadActivities = vi.fn().mockResolvedValue({
      data: [activity],
      total: 100,
      limit: 50,
      hasMore: true,
      nextCursor: 'older',
    });
    const loadTransactions = vi
      .fn()
      .mockResolvedValue({ data: [], total: 0, limit: 50, hasMore: false, nextCursor: null });
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
      expect(loadActivities).toHaveBeenLastCalledWith({ limit: 50, cursor: '100:2' })
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith('older'));
    fireEvent.click(await screen.findByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith('100:2'));
    fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
    await waitFor(() =>
      expect(loadTransactions).toHaveBeenCalledWith({ limit: 50, cursor: undefined })
    );
  });
});
