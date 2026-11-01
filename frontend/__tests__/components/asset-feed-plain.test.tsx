// F2 group `objects` (round 2): the asset boards' activity rows — plain rows without script-call
// lines and with the lock kind as its own column (OB-12, IV-14), a batch of objects folded behind
// "+N more objects" that leads with this page's object (OB-M3), and the item Transactions column
// naming where the object went (OB-13).
import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { AssetFeed } from '@/components/asset/asset-feed';
import { objectCountLabel } from '@/components/activity/activity-row';
import { ItemTransactionContext } from '@/components/asset/item-evidence';
import type { GlobalActivity, Transaction } from '@/lib/api';
import {
  itemTransactionsProducer,
  sporeCellVersionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';

const obj = (n: number, delta: 1 | -1) => ({
  kind: 'object' as const,
  objectId: `0x${n.toString(16).padStart(2, '0').repeat(28)}`,
  delta,
  collection: { id: '0x' + '77'.repeat(24), name: null },
  class: 'object' as const,
  composition: null,
});
const party = (lock: string, kind: string, delta: string, items: ReturnType<typeof obj>[]) => ({
  lockHash: '0x' + lock.repeat(32),
  address: null,
  roles: [],
  lockHashPrefix: null,
  lockKind: kind,
  tags: 4,
  ckbDelta: delta,
  usedDelta: '0',
  itemDeltas: items,
});
const batch: GlobalActivity = {
  txHash: '0x' + 'cd'.repeat(32),
  blockNumber: 12,
  txIndex: 3,
  timestamp: '1700000000000',
  isCellbase: false,
  participants: [
    party('01', 'UTXOSwap Intent Lock', '-100000', [obj(1, -1), obj(2, -1), obj(3, -1)]),
    party('02', 'PW Lock', '0', [obj(1, 1), obj(2, 1), obj(3, 1)]),
  ],
  typeCalls: [
    {
      typeCodeHash: '0x' + '0a'.repeat(32),
      typeHashType: 'type',
      typeArgs: '0x1234',
      scriptHash: '0x' + '0b'.repeat(32),
      scriptName: 'M-NFT',
    },
  ],
  lockCalls: [
    {
      lockCodeHash: '0x' + '0c'.repeat(32),
      lockHashType: 'type',
      lockArgs: '0x5678',
      scriptHash: '0x' + '0d'.repeat(32),
      scriptName: 'PW Lock',
    },
  ],
  protocolActions: [],
};
const page = { data: [batch], total: 1, limit: 50, hasMore: false, nextCursor: null };

function feed(plain: boolean) {
  return render(
    <AssetFeed
      queryKey={`plain-${plain}`}
      scope="Its whole life"
      plain={plain}
      focusItemId={obj(2, 1).objectId}
      loadActivities={async () => page}
      loadTransactions={async () => ({ ...page, data: [] })}
    />
  );
}

it('drops the script-call lines and gives the lock kind its own column (OB-12, IV-14)', async () => {
  const { container } = feed(true);
  await screen.findAllByText(/more objects/);
  expect(container.querySelector('[data-role="detail"]')).toBeNull();
  const kinds = container.querySelectorAll('[data-role="lock-kind"]');
  expect([...kinds].map((k) => k.textContent)).toEqual(['UTXOSwap Intent Lock', 'PW Lock']);
  expect(kinds[0]).toHaveClass('whitespace-nowrap');
});

it('keeps the script-call lines on a feed that does not ask for plain rows', async () => {
  const { container } = feed(false);
  expect(await screen.findAllByText(/0202…|object/)).not.toHaveLength(0);
  expect(container.querySelector('[data-role="detail"]')).not.toBeNull();
  expect(container.querySelector('[data-role="lock-kind"]')).toBeNull();
});

it('folds a batch of objects behind "+N more objects", leading with this one (OB-M3)', async () => {
  const { container } = feed(true);
  const more = await screen.findAllByRole('button', { name: '+2 more objects' });
  expect(more).toHaveLength(2);
  const first = container.querySelector('[data-role="participants"]')!;
  expect(first.querySelectorAll('a[href*="/objects/"]')).toHaveLength(2);
  expect(first.innerHTML).toContain(obj(2, 1).objectId.slice(0, 10));
  fireEvent.click(more[0]);
  expect(screen.getAllByRole('button', { name: '+2 more objects' })).toHaveLength(1);
  expect(first.querySelectorAll('a[href*="/objects/"]').length).toBe(4);
});

// L6 (responsive plan, B#12): below md a participant is one line — party, lock, a count chip for
// its objects that opens them in place, the amount and occupied — and every participant is kept.
describe('phone form of a collection activity entry (L6)', () => {
  it("counts a participant's objects in one signed chip that opens them in place", async () => {
    const { container } = feed(true);
    const chips = await screen.findAllByRole('button', { name: /^[+−]\d+ objects?$/ });
    expect(chips.map((c) => c.textContent)).toEqual(['−3 objects', '+3 objects']);
    expect(chips[0].closest('[data-role="phone-items"]')).toHaveClass('md:hidden');
    expect(chips[0]).toHaveAttribute('aria-expanded', 'false');
    const first = container.querySelectorAll('[data-role="participant"]')[0];
    // The objects' own lines are the md form only; the phone shows the chip.
    const mdItems = first.querySelector('[data-role="phone-items"] ~ div:last-child')!;
    expect(mdItems).toHaveClass('hidden', 'md:block');
    expect(first.querySelector('[data-role="phone-objects"]')).toBeNull();
    fireEvent.click(chips[0]);
    expect(chips[0]).toHaveAttribute('aria-expanded', 'true');
    const opened = first.querySelector('[data-role="phone-objects"]')!;
    expect(opened).toHaveClass('basis-full', 'md:hidden');
    expect(opened.querySelectorAll('a[href*="/objects/"]')).toHaveLength(3);
    // Every participant stays (two here, both on their own line).
    expect(container.querySelectorAll('[data-role="participant"]')).toHaveLength(2);
  });

  it("keeps occupied on the participant's line in the plain rows only", async () => {
    const withOccupied = {
      ...batch,
      participants: batch.participants.map((p) => ({ ...p, usedDelta: '32400000000' })),
    };
    const { container } = render(
      <AssetFeed
        queryKey="plain-occupied"
        scope="Its whole life"
        plain
        loadActivities={async () => ({ ...page, data: [withOccupied] })}
        loadTransactions={async () => ({ ...page, data: [] })}
      />
    );
    await screen.findAllByRole('button', { name: /objects?$/ });
    const occupied = container.querySelector('[data-role="occupied"]')!;
    expect(occupied).toHaveTextContent('occupied +324');
    expect(occupied).not.toHaveClass('basis-full');
  });

  it('writes the chip as a signed count', () => {
    expect(objectCountLabel([obj(1, 1)])).toBe('+1 object');
    expect(objectCountLabel([obj(1, 1), obj(2, -1)])).toBe('+1 −1 objects');
  });
});

it('says where the object went in the Transactions column (OB-13)', () => {
  const row = itemTransactionsProducer.data[0] as Transaction & {
    itemContext: NonNullable<(typeof itemTransactionsProducer.data)[0]['itemContext']>;
  };
  const created = { ...sporeCellVersionsProducer.data[0], outputIndex: 1 };
  render(
    <ItemTransactionContext
      transaction={{ ...row, itemContext: { ...row.itemContext, createdCell: created } }}
      item={{ kind: 'object' }}
    />
  );
  expect(screen.getByText(/^to /)).toHaveTextContent('(output 1)');
  expect(screen.getByRole('link', { name: /ckb1qpq5zs…/ })).toHaveAttribute(
    'href',
    expect.stringContaining(`/address/${created.ownerAddress}`)
  );
});

// Round 3 (contracts R-A · R-4, OB-M10): the m-NFT mint row says who holds the class cell.
describe('m-NFT mint row class cell (R-4)', () => {
  const mint = (classCell: unknown) => ({
    ...batch,
    itemContext: {
      itemId: obj(1, 1).objectId,
      action: 'mint',
      protocolActions: [],
      ringRelink: null,
      actedOn: 'self',
      createdCell: null,
      consumedCell: null,
      versionDiff: null,
      classCell,
    },
  });
  const itemFeed = (row: unknown) =>
    render(
      <AssetFeed
        queryKey={`class-cell-${JSON.stringify(row).length}`}
        scope="Its whole life"
        plain
        item={{ kind: 'object' }}
        loadActivities={async () => ({ ...page, data: [row as GlobalActivity] })}
        loadTransactions={async () => ({ ...page, data: [] })}
      />
    );

  it('notes "holds the class cell" on the participant whose lock holds it', async () => {
    const { container } = itemFeed(
      mint({ outputIndex: 1, lockHash: '0x' + '02'.repeat(32), address: 'ckb1qclass' })
    );
    await screen.findAllByText(/more objects/);
    const notes = container.querySelectorAll('[data-role="participant-note"]');
    expect([...notes].map((n) => n.textContent)).toEqual(['holds the class cell']);
    expect(notes[0].closest('[data-party]')).toHaveAttribute(
      'data-party',
      `0x${'02'.repeat(32)}:1`
    );
  });

  it('says nothing when the mint outputs no cell of its class', async () => {
    const { container } = itemFeed(mint(null));
    await screen.findAllByText(/more objects/);
    expect(container.querySelector('[data-role="participant-note"]')).toBeNull();
  });

  it('reports a class cell held by no participant in place', async () => {
    itemFeed(mint({ outputIndex: 1, lockHash: '0x' + '09'.repeat(32), address: 'ckb1qclass' }));
    expect(await screen.findByTestId('activity-read-error')).toHaveTextContent(
      `the class cell's lock 0x${'09'.repeat(32)} is no participant`
    );
  });
});
