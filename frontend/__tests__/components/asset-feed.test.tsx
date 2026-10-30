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
  it('prefers itemContext.action over net identity mint inference (ID-C1)', () => {
    const row = {
      ...activity,
      itemContext: {
        itemId: '0x' + '11'.repeat(20),
        action: 'update',
        createdCell: null,
        consumedCell: null,
      },
      tags: 4, // TAG_IDENTITY
      participants: [
        {
          ...activity.participants[0],
          tags: 4,
          itemDeltas: [
            {
              kind: 'identity',
              standard: 'dotbit',
              identityId: '0x' + '11'.repeat(20),
              delta: 1,
            },
          ],
        },
      ],
    } as ItemActivity;
    expect(itemFeedHeadline(row, 'identity')).toEqual({
      text: 'Identity Update',
      neighbour: false,
    });
  });

  const dotcell = (action: string, metadata: ItemActivity['protocolActions'][number]['metadata']) =>
    ({
      ...activity,
      participants: [{ ...activity.participants[0], itemDeltas: [] }],
      protocolActions: [{ protocol: 'dotcell', protocolName: '.cell', action, metadata }],
      itemContext: { itemId: aptId, action: 'update', createdCell: null, consumedCell: null },
    }) as ItemActivity;
  const aptId = '0x' + 'a7'.repeat(20);

  it('keeps the .cell protocol action names over the coarse item action (ID-C1)', () => {
    expect(itemFeedHeadline(dotcell('edit_records', { label: 'apt' }), 'identity')).toEqual({
      text: '.cell · edit records',
      neighbour: false,
    });
    expect(itemFeedHeadline(dotcell('transfer', { label: 'apt' }), 'identity').text).toBe(
      '.cell · transfer'
    );
  });

  it('names an identity row without protocol actions or deltas by its item action, not "Item"', () => {
    const did = {
      ...activity,
      participants: [{ ...activity.participants[0], itemDeltas: [] }],
      itemContext: {
        itemId: '0x' + '12'.repeat(20),
        action: 'update',
        createdCell: null,
        consumedCell: null,
      },
    } as ItemActivity;
    expect(itemFeedHeadline(did, 'identity').text).toBe('Identity Update');
    expect(() =>
      itemFeedHeadline({ ...did, itemContext: { ...did.itemContext, action: '' } }, 'identity')
    ).toThrow('item feed row has no item action');
  });

  it('dims a ring neighbour registration on the predecessor it relinked', async () => {
    const register = dotcell('register', { label: 'ckb', relinked: aptId.toUpperCase() });
    expect(itemFeedHeadline(register, 'identity')).toEqual({
      text: 'Neighbour registered',
      neighbour: true,
    });
    // The same registration on the new name's own page is its registration.
    const own = {
      ...register,
      itemContext: { ...register.itemContext, itemId: '0x' + 'cc'.repeat(20) },
    };
    expect(itemFeedHeadline(own, 'identity')).toEqual({
      text: '.cell · register',
      neighbour: false,
    });
    expect(() =>
      itemFeedHeadline(dotcell('register', { relinked: 'apt.cell' }), 'identity')
    ).toThrow('invalid .cell relink');
    render(
      <AssetFeed
        queryKey="dotcell:apt"
        scope="This identity's whole life"
        itemKind="identity"
        loadActivities={async () => ({
          data: [register],
          limit: 50,
          hasMore: false,
          nextCursor: null,
        })}
        loadTransactions={async () => ({ data: [], limit: 50, hasMore: false, nextCursor: null })}
      />
    );
    const headline = await screen.findByText('Neighbour registered');
    expect(headline.closest('article')).toHaveClass('opacity-[0.45]');
    expect(screen.queryByText('.cell · register')).not.toBeInTheDocument();
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
