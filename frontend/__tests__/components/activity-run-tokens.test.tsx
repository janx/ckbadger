// Ruling 6 of gate P (the owner, 2026-11-20): tokens of one standard fold into one row ("Token
// transfer · Simple UDT" holds USDC and USDC|bsc), and the folded row says how many tokens its rows
// moved beside its transactions and its participants: "2 tokens". Distinct tokens by the token's
// identity as the rows carry it, its type script's hash (an item delta's `typeScriptHash`, the id
// the token's page is keyed by: two tokens of one symbol are two), counted over the run's rows by
// the same paging as its other counts ("so far" while older rows are unread), on every folded token
// run, one token too.
import { describe, expect, it } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { addressActivitiesFixture, addressFixture } from '@/__tests__/msw/fixtures/address';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { AddressActivities } from '@/components/address/address-history';
import type { Activity, GlobalActivity, PartyId } from '@/lib/api';
import { activityRead, ITEM_TYPE, type WrittenParty } from '@/__tests__/utils/activity-read';

const USDC = `0x${'7e'.repeat(32)}`;
const USDC_BSC = `0x${'7f'.repeat(32)}`;
/** Another token that calls itself USDC: another type script, another token. */
const USDC_OTHER = `0x${'80'.repeat(32)}`;

const token = (hash: string, symbol: string, delta: string) => ({
  kind: 'token' as const,
  typeScriptHash: hash,
  delta,
  symbol,
  decimals: 6,
});

const party = (
  seed: string,
  ckbDelta: string,
  items: ReturnType<typeof token>[]
): WrittenParty => ({
  address: seededAddress(seed),
  lockHash: `0x${seed.padStart(64, '0')}`,
  lockHashPrefix: null,
  lockKind: 'Default Lock',
  roles: [],
  tags: 1,
  ckbDelta,
  usedDelta: '0',
  itemDeltas: items,
});

/** A token transfer of one token from `from` to `to`, as the route sends it. */
function transfer(
  n: number,
  hash: string,
  symbol: string,
  from: string,
  to: string
): GlobalActivity {
  return activityRead({
    txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
    blockNumber: 21_016_902 - n,
    txIndex: 1,
    timestamp: String(Date.now() - n * 60_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    itemTypes: [ITEM_TYPE.sudt],
    itemTypesUnread: null,
    participants: [
      party(from, '-1000', [token(hash, symbol, '-5')]),
      party(to, '990', [token(hash, symbol, '5')]),
    ],
  });
}

function feed(rows: GlobalActivity[], more = false) {
  return render(
    <ActivityRuns
      more={more}
      rows={rows.map((a) => ({
        key: a.txHash,
        subject: runSubject(a, activityTitleText(a)),
        activity: a,
        title: <ActivityTitle activity={a} />,
        row: <ActivityRow activity={a} />,
      }))}
    />
  );
}

const countLine = () =>
  (document.querySelector('[data-role="run-count"]') as HTMLElement).textContent!.replace(
    /\s+/g,
    ' '
  );

describe('a folded token run counts its tokens', () => {
  it('counts two tokens of one standard, each once', () => {
    feed([
      transfer(1, USDC, 'USDC', 'aaaaaa', 'bbbbbb'),
      transfer(2, USDC_BSC, 'USDC|bsc', 'aaaaaa', 'cccccc'),
      transfer(3, USDC, 'USDC', 'bbbbbb', 'cccccc'),
    ]);
    expect(countLine()).toBe('all 3 transactions ↓ · 3 participants · 2 tokens');
  });

  it('says one token too', () => {
    feed([
      transfer(1, USDC, 'USDC', 'aaaaaa', 'bbbbbb'),
      transfer(2, USDC, 'USDC', 'aaaaaa', 'cccccc'),
    ]);
    expect(countLine()).toBe('all 2 transactions ↓ · 3 participants · 1 token');
  });

  it('counts tokens by their type script, not their symbol', () => {
    feed([
      transfer(1, USDC, 'USDC', 'aaaaaa', 'bbbbbb'),
      transfer(2, USDC_OTHER, 'USDC', 'aaaaaa', 'bbbbbb'),
    ]);
    expect(countLine()).toBe('all 2 transactions ↓ · 2 participants · 2 tokens');
  });

  it('says so far of the tokens of a run older rows may go on', () => {
    feed(
      [
        transfer(1, USDC, 'USDC', 'aaaaaa', 'bbbbbb'),
        transfer(2, USDC_BSC, 'USDC|bsc', 'aaaaaa', 'bbbbbb'),
      ],
      true
    );
    expect(countLine()).toBe('2 transactions so far ↓ · 2 participants so far · 2 tokens so far');
  });

  it('counts no token on a run that moved none', () => {
    const call = (n: number): GlobalActivity =>
      activityRead({
        txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
        blockNumber: 21_047_791 - n,
        txIndex: 1,
        timestamp: String(Date.now() - n * 49_000),
        isCellbase: false,
        protocolActions: [],
        typeCalls: [
          {
            typeCodeHash: `0x${'3a'.repeat(32)}`,
            typeHashType: 'type',
            typeArgs: '0x00',
            scriptHash: `0x${'3a'.repeat(31)}00`,
            scriptName: '.bit Time Index State',
          },
        ],
        lockCalls: [],
        itemTypes: [],
        itemTypesUnread: null,
        participants: [{ ...party('q3q7ue', '-30000', []), tags: 0 }],
      });
    feed([call(1), call(2)]);
    expect(countLine()).toBe('all 2 transactions ↓ · 1 participant');
  });

  // an address's run counts the tokens the address moved, the items its row carries
  it('counts an address’s own tokens on its page', () => {
    const own = [addressFixture.lockScriptHash.slice(0, 42), 1] as PartyId;
    const received = (n: number, hash: string, symbol: string): Activity =>
      activityRead({
        ...addressActivitiesFixture[0],
        txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
        blockNumber: 21_016_902 - n,
        timestamp: String(Date.now() - n * 60_000),
        ckbDelta: '0',
        usedDelta: '0',
        tags: 1,
        itemTypes: [ITEM_TYPE.sudt],
        itemDeltas: [{ ...token(hash, symbol, '5'), movement: 'transfer' }],
        participantCount: 2,
        partyIds: [own, [`0x${'aa'.repeat(20)}`, 1]],
        feePayer: null,
      });
    render(
      <AddressActivities
        more={false}
        rows={[
          received(1, USDC, 'USDC'),
          received(2, USDC_BSC, 'USDC|bsc'),
          received(3, USDC, 'USDC'),
        ]}
        address={addressFixture}
      />
    );
    expect(countLine()).toBe('all 3 transactions ↓ · 2 participants · 2 tokens');
  });
});
