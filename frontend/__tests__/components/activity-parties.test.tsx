// Lane feed, ruling 14 of gate P (the owner, 2026-11-20): /activities?filter=token answered 8.25 MB
// for one page of 50, every field of each of a Godwoken row's 600 to 770 parties. The route now
// states each activity's exact `participantCount`, sends in full only the parties a row draws, names
// every party by its short id with its tags (`partyIds`), and states what the page derived from all
// of them: the fee payer (`feePayer`) and each item's movement in the whole transaction. The page
// reads those statements and derives nothing from a list it knows is partial; it refuses a response
// without them, as an API older than the page.
import { describe, expect, it } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { addressActivitiesFixture, addressFixture } from '@/__tests__/msw/fixtures/address';
import { ActivityRow } from '@/components/activity/activity-row';
import { runSubject } from '@/components/activity/activity-runs';
import { activityHeadline } from '@/components/activity-event-row';
import { AddressActivity } from '@/components/address/address-history';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import { runParties } from '@/lib/activity-runs';
import type { Activity, GlobalActivity, PartyId, Transaction } from '@/lib/api';
import {
  activityRead,
  ITEM_TYPE,
  partyShortId,
  type WrittenParty,
} from '@/__tests__/utils/activity-read';

const party = (seed: string, ckbDelta: string, over: Partial<WrittenParty> = {}): WrittenParty => ({
  address: seededAddress(seed),
  lockHash: `0x${seed.padStart(64, '0')}`,
  lockHashPrefix: null,
  lockKind: 'Default Lock',
  roles: [],
  tags: 0,
  ckbDelta,
  usedDelta: '0',
  itemDeltas: [],
  ...over,
});
const idOf = (p: WrittenParty): PartyId => [partyShortId(p, 'test'), p.tags];
const hashOf = (n: number) => `0x${String(n).padStart(2, '0').repeat(32)}`;

/**
 * A row as the route sends one that draws `drawn` of the parties `named` (every party of the
 * transaction, in its order), with the route's fee payer.
 */
function routeRow(
  n: number,
  drawn: WrittenParty[],
  named: WrittenParty[],
  feePayer: string | null = null,
  itemTypes: GlobalActivity['itemTypes'] = [],
  items: GlobalActivity['items'] = []
): GlobalActivity {
  return activityRead({
    txHash: hashOf(n),
    blockNumber: 21_000_000 - n,
    txIndex: 1,
    timestamp: String(Date.now() - n * 60_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    itemTypes,
    itemTypesUnread: null,
    participants: drawn,
    participantCount: named.length,
    partyIds: named.map(idOf),
    feePayer,
    items,
  });
}

/** `count` parties of a Godwoken-like row that changed a few shannons each and moved no item. */
const crowd = (count: number, from: number) =>
  Array.from({ length: count }, (_, i) => party(`c${String(from + i).padStart(5, '0')}`, '1000'));

/** The row's response as an API older than this page sends it: without `field`. */
function without<R extends object>(row: R, field: string): R {
  return Object.fromEntries(Object.entries(row).filter(([key]) => key !== field)) as R;
}

const lines = (c: HTMLElement) => c.querySelectorAll('[data-role="participant"]');

describe('the fold counts every party a run names, drawn or not', () => {
  // a party drawn in one row and only listed in another counts once; a party listed and never
  // drawn counts too (the fold read the drawn parties alone: 2 here)
  it('reads the parties from the route’s id list', () => {
    const [x, y, z] = [party('aaaaaa', '-1000'), party('bbbbbb', '500'), party('cccccc', '500')];
    const a = routeRow(1, [x], [x, y]);
    const b = routeRow(2, [z], [x, z]);
    expect(runParties([runSubject(a, 'CKB Transfer')!, runSubject(b, 'CKB Transfer')!])).toBe(3);
  });
});

describe('the page reads what the route states of every party', () => {
  const USDC = `0x${'7e'.repeat(32)}`;
  const usdc = (delta: string, movement: 'mint' | 'burn' | 'transfer') => ({
    kind: 'token' as const,
    typeScriptHash: USDC,
    delta,
    symbol: 'USDC',
    decimals: 6,
    movement,
  });

  // the route states the item's movement from every party; the page sums nothing (its two drawn
  // amounts here net to zero, which it read as "transfer")
  it('heads a token row by the movement the route states', () => {
    const giver = party('aaaaaa', '-1', { tags: 1, itemDeltas: [usdc('-5', 'mint')] });
    const taker = party('bbbbbb', '1', { tags: 1, itemDeltas: [usdc('5', 'mint')] });
    const row = routeRow(3, [giver, taker], [giver, taker], null, [ITEM_TYPE.sudt]);
    expect(activityHeadline(row)).toBe('Token mint');
  });

  // the route says no party paid the fee alone; the page drew "fee" from the parties it was sent
  it('says no fee where the route names no fee payer', () => {
    const payer = party('aaaaaa', '-30000');
    const row = routeRow(4, [payer, party('bbbbbb', '0')], [payer, party('bbbbbb', '0')], null);
    const { container } = render(<ActivityRow activity={row} />);
    expect(container.querySelector('[data-fee]')).toBeNull();
  });

  it('says the fee on the party the route names', () => {
    // eight parties, the payer's the one change: it is drawn, beside the four earliest of the rest
    const payer = party('aaaaaa', '-30000');
    const zero = (n: number) => party(`00000${n}`, '0');
    const named = [zero(1), zero(2), payer, zero(3), zero(4), zero(5), zero(6), zero(7)];
    const drawn = named.slice(0, 5);
    const row = routeRow(5, drawn, named, payer.lockHash);
    const { container } = render(<ActivityRow activity={row} />);
    const fee = container.querySelector('[data-fee]')!;
    expect(fee).toHaveTextContent(/^fee 0\.0003 CKB$/);
  });

  // an address's row draws its own party alone; the route says who paid only the fee, from every
  // party, so the row says "fee" where the transaction has other parties too (it could not: their
  // changes were not on the row)
  it('says the fee on an address’s row where the route names the address', () => {
    const own = [addressFixture.lockScriptHash.slice(0, 42), 0] as PartyId;
    const other = party('other1', '0');
    const row: Activity = activityRead({
      ...addressActivitiesFixture[0],
      ckbDelta: '-30000',
      usedDelta: '0',
      participantCount: 2,
      partyIds: [own, idOf(other)],
      feePayer: addressFixture.lockScriptHash,
    });
    const { container } = render(<AddressActivity activity={row} address={addressFixture} />);
    expect(container.querySelector('[data-fee]')).toHaveTextContent(/^fee 0\.0003 CKB$/);
  });

  // the address's own change is the only one its row draws; the route names no fee payer (another
  // party received): no "fee" (the row drew it from the one line it had)
  it('says no fee on an address’s row where the route names no fee payer', () => {
    const own = [addressFixture.lockScriptHash.slice(0, 42), 0] as PartyId;
    const row: Activity = activityRead({
      ...addressActivitiesFixture[0],
      ckbDelta: '-30000',
      usedDelta: '0',
      participantCount: 2,
      partyIds: [own, idOf(party('other1', '29000'))],
      feePayer: null,
    });
    const { container } = render(<AddressActivity activity={row} address={addressFixture} />);
    expect(container.querySelector('[data-fee]')).toBeNull();
  });

  it('counts a transaction list row’s parties by the route’s count', () => {
    const [x, y] = [party('aaaaaa', '-1000'), party('bbbbbb', '990')];
    const row = routeRow(6, [x, y], [x, y, ...crowd(768, 0)]);
    const tx: Transaction = {
      hash: row.txHash,
      blockNumber: row.blockNumber,
      blockHash: `0x${'cd'.repeat(32)}`,
      index: 1,
      inputsCount: 3,
      outputsCount: 3,
      fee: '10',
      value: '990',
      outputCapacity: '100000000000',
      txSize: 903,
      cycles: 1704957,
      cyclesStatus: 'complete',
      timestamp: '2026-10-25T00:00:00Z',
      isCellbase: false,
      activity: row,
    };
    render(<TransactionListTable transactions={[tx]} />);
    expect(screen.getByRole('table')).toHaveTextContent('770 participants');
  });
});

describe('a row that draws some of its parties says how many it does not', () => {
  const big = (n: number, drawnCount: number) => {
    const drawn = [
      party('aaaaaa', '-500000000000'),
      ...Array.from({ length: drawnCount - 1 }, (_, i) =>
        party(`d${String(i).padStart(5, '0')}`, String(100_000_000_000 - i))
      ),
    ];
    return { row: routeRow(n, drawn, [...drawn, ...crowd(770 - drawnCount, 0)]), drawn };
  };

  it('draws six of 770 whole and says the other 764 are on the transaction’s page', () => {
    const { row } = big(7, 6);
    const { container } = render(<ActivityRow activity={row} />);
    expect(lines(container)).toHaveLength(6);
    expect(screen.queryByRole('button', { name: /participants/ })).toBeNull();
    const rest = screen.getByRole('link', {
      name: /764\smore\sparticipants on the transaction page/,
    });
    expect(rest).toHaveAttribute('href', expect.stringContaining(`/tx/${row.txHash}`));
  });

  it('folds eight of 770 to five, opens the eight, then says the other 762', async () => {
    const user = userEvent.setup();
    const { row } = big(8, 8);
    const { container } = render(<ActivityRow activity={row} />);
    expect(lines(container)).toHaveLength(5);
    expect(screen.queryByRole('link', { name: /more\sparticipants/ })).toBeNull();
    await user.click(screen.getByRole('button', { name: /^8 of 770\sparticipants ↓$/ }));
    expect(lines(container)).toHaveLength(8);
    expect(screen.getByRole('button', { name: /^fewer ↑$/ })).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: /762\smore\sparticipants on the transaction page/ })
    ).toBeInTheDocument();
  });

  it('keeps a row that draws every party as it was: all of them behind "all 8 participants"', async () => {
    const user = userEvent.setup();
    const drawn = [party('aaaaaa', '-8000'), ...crowd(7, 0)];
    const { container } = render(<ActivityRow activity={routeRow(9, drawn, drawn)} />);
    expect(lines(container)).toHaveLength(5);
    await user.click(screen.getByRole('button', { name: /^all 8\sparticipants ↓$/ }));
    expect(lines(container)).toHaveLength(8);
    expect(screen.queryByRole('link', { name: /more\sparticipants/ })).toBeNull();
  });

  it('names no rest on an address’s row, which draws its own party alone', () => {
    const own = [addressFixture.lockScriptHash.slice(0, 42), 0] as PartyId;
    const row: Activity = activityRead({
      ...addressActivitiesFixture[0],
      participantCount: 770,
      partyIds: [own, ...crowd(769, 0).map(idOf)],
      feePayer: null,
    });
    const { container } = render(<AddressActivity activity={row} address={addressFixture} />);
    expect(lines(container)).toHaveLength(1);
    expect(within(container).queryByRole('link', { name: /more\sparticipants/ })).toBeNull();
  });
});

describe('a response from an API older than this page is refused by name', () => {
  const [x, y] = [party('aaaaaa', '-1000'), party('bbbbbb', '990')];
  const row = routeRow(10, [x, y], [x, y]);

  it('refuses a row without its id list where a run is compared', () => {
    expect(() => runSubject(without(row, 'partyIds'), 'CKB Transfer')).toThrow(
      `transaction ${row.txHash}: the API sends no partyIds: it is older than this page`
    );
  });

  it('refuses a row without its count where it is drawn', () => {
    expect(() => render(<ActivityRow activity={without(row, 'participantCount')} />)).toThrow(
      `transaction ${row.txHash}: the API sends no participantCount: it is older than this page`
    );
  });

  it('refuses a row without its fee payer where it is drawn', () => {
    expect(() => render(<ActivityRow activity={without(row, 'feePayer')} />)).toThrow(
      `transaction ${row.txHash}: the API sends no feePayer: it is older than this page`
    );
  });

  it('refuses an item without its movement where a headline reads it', () => {
    const usdc = { kind: 'token' as const, typeScriptHash: `0x${'7e'.repeat(32)}`, delta: '5' };
    const moved = routeRow(
      11,
      [{ ...x, tags: 1, itemDeltas: [{ ...usdc, movement: 'mint' }] }],
      [x],
      null,
      [ITEM_TYPE.sudt]
    );
    // the row as an older API sends it: its item without the movement
    const older = structuredClone(moved);
    delete (older.participants[0].itemDeltas[0] as { movement?: string }).movement;
    expect(() => activityHeadline(older)).toThrow(
      `transaction ${moved.txHash}: the API sends no movement: it is older than this page`
    );
  });
});
