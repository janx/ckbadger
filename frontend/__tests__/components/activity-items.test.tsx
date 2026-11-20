// Lane feed, part 4 (the owner's "做", 2026-11-20): a row draws no party for moving an item (on
// Godwoken rows hundreds of parties' items move); the route states the transaction's items once,
// each from every party: what it is, its movement, how many parties' holdings changed and a token's
// amount moved. The page derives no fact of a row's items from the parties it draws: the headline,
// the fold's token count and an asset feed's amount moved read the statement, and a response
// without it is refused, as an API older than the page.
import { describe, expect, it } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { activityHeadline } from '@/components/activity-event-row';
import { tokenMoved } from '@/components/asset/asset-feed';
import type { GlobalActivity, PartyId, RowItem, Transaction } from '@/lib/api';
import {
  activityRead,
  ITEM_TYPE,
  partyId,
  type WrittenParty,
} from '@/__tests__/utils/activity-read';

const USDC = `0x${'7e'.repeat(32)}`;
const USDC_BSC = `0x${'7f'.repeat(32)}`;

/** A party that moved CKB alone: the parties a row draws move none of its items here. */
const party = (seed: string, ckbDelta: string): WrittenParty => ({
  address: seededAddress(seed),
  lockHash: `0x${seed.padStart(64, '0')}`,
  lockHashPrefix: null,
  lockKind: 'Default Lock',
  roles: [],
  tags: 0,
  // it sent where its CKB fell (exact that way round), as the route states `sent` (resync/int)
  sent: BigInt(ckbDelta) < BigInt(0),
  ckbDelta,
  usedDelta: '0',
  itemDeltas: [],
});
/** The party as the route names it: its id and its tags, the senders bit where it sent. */
const idOf = (p: WrittenParty): PartyId => partyId(p, 'test');

const token = (hash: string, symbol: string, movement: RowItem['movement'], amount: string) =>
  ({
    kind: 'token',
    typeScriptHash: hash,
    symbol,
    decimals: 6,
    movement,
    amount,
    parties: 300,
  }) as RowItem;

/**
 * A Godwoken-like row as the route sends it: 300 parties, the five largest CKB changes drawn
 * (none moved an item), its items stated.
 */
function godwoken(n: number, items: RowItem[]): GlobalActivity {
  const drawn = [0, 1, 2, 3, 4].map((i) =>
    party(`g${n}${i}`.padEnd(6, '0'), `-${9 - i}00000000000`)
  );
  const named = [
    ...drawn,
    ...Array.from({ length: 295 }, (_, i) => party(`r${n}${String(i).padStart(4, '0')}`, '1000')),
  ];
  return activityRead({
    txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
    blockNumber: 21_016_902 - n,
    txIndex: 1,
    timestamp: String(Date.now() - n * 60_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    recognizedTypes: [ITEM_TYPE.sudt],
    participants: drawn,
    participantCount: named.length,
    partyIds: named.map(idOf),
    feePayer: null,
    items,
  });
}

/** The row's response as an API older than this page sends it: without `items`. */
const older = (row: GlobalActivity) =>
  Object.fromEntries(Object.entries(row).filter(([key]) => key !== 'items')) as GlobalActivity;

describe('the page reads a row’s items from the route’s statement', () => {
  it('heads the row by its stated item, which none of its drawn parties moved', () => {
    const row = godwoken(1, [token(USDC, 'USDC', 'mint', '500000000')]);
    expect(activityHeadline(row)).toBe('Token mint');
    const { container } = render(<ActivityRow activity={row} />);
    expect(container.querySelector('[data-role="headline"]')).toHaveTextContent('Token mint');
  });

  it('counts a folded run’s tokens from its rows’ stated items', () => {
    const rows = [
      godwoken(1, [token(USDC, 'USDC', 'transfer', '5')]),
      godwoken(2, [
        token(USDC, 'USDC', 'transfer', '7'),
        token(USDC_BSC, 'USDC|bsc', 'transfer', '2'),
      ]),
    ];
    render(
      <ActivityRuns
        more={false}
        rows={rows.map((a) => ({
          key: a.txHash,
          subject: runSubject(a, activityTitleText(a)),
          activity: a,
          title: <ActivityTitle activity={a} />,
          row: <ActivityRow activity={a} />,
        }))}
      />
    );
    expect(
      (document.querySelector('[data-role="run-count"]') as HTMLElement).textContent!.replace(
        /\s+/g,
        ' '
      )
      // re-pinned on purpose (resync/int, at the merge of lane feed): the run counts its senders
      // (the owner's word at Gate B), each row's five drawn parties whose CKB fell, from the senders
      // bit of the route's id list; it counted all 600 parties
    ).toBe('all 2 transactions ↓ · 10 senders · 2 tokens');
  });

  it('reads an asset feed’s amount moved from the statement', () => {
    const activity = godwoken(3, [token(USDC, 'USDC', 'burn', '1234567890123456789012345678901')]);
    const tx = { hash: activity.txHash, activity } as Transaction;
    expect(tokenMoved(tx, USDC)).toBe('1234567890123456789012345678901');
  });

  it('refuses a row without its stated items, where the headline, a run and an amount read them', () => {
    const row = godwoken(4, [token(USDC, 'USDC', 'mint', '5')]);
    const said = `transaction ${row.txHash}: the API sends no items: it is older than this page`;
    expect(() => activityHeadline(older(row))).toThrow(said);
    expect(() => runSubject(older(row), 'tokens: Token mint')).toThrow(said);
    expect(() =>
      tokenMoved({ hash: row.txHash, activity: older(row) } as Transaction, USDC)
    ).toThrow(said);
  });
});
