// Readability 2026-11-18, lane B4 (pG) item 5, with the rule the owner ruled at Gate B (lists lane
// P3): consecutive rows that did the same thing and called the same script code (not the args, not
// the parties) become one row that says what they called, how many transactions and how many
// senders (resync/int: the parties whose lock is an input's; it said participants), and the newest
// time, opened in place. It counts and sums nothing: the folded row
// shows no amount and no total. The rule of "the same" is lib/activity-runs.ts (its own test); this
// pins how the feeds draw a run.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { AssetFeed } from '@/components/asset/asset-feed';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runMemory, runSubject } from '@/components/activity/activity-runs';
import type { GlobalActivity, ItemActivity, ParticipantInfo, PartyId } from '@/lib/api';
import { api, TAG_INPUT, TAG_TYPE_CALL } from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';
import { liveFeeds } from '@/__tests__/utils/live-activities';
import { ROW_LINE_PX } from '@/lib/type-metrics';
import { SCREENS } from '@/__tests__/utils/tailwind-box';
import resolveConfig from 'tailwindcss/resolveConfig';
import tailwindConfig from '@/tailwind.config';
import { pagedRead } from '@/__tests__/utils/paged-read';
import { activityRead, ITEM_TYPE, unsentRow } from '@/__tests__/utils/activity-read';

vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, api: { ...actual.api, getGlobalActivities: vi.fn() } };
});

class NoopObserver {
  observe() {}
  disconnect() {}
  unobserve() {}
  takeRecords() {
    return [];
  }
}

/** The indexer's TAG_TOKEN (crates/ckbadger-store/src/types.rs, bit 0): the party moved a token. */
const TAG_TOKEN = 1;

const NOW = Date.parse('2026-11-18T08:00:00Z');

/** An oracle's party: it spends its own cell to write the next one, so it sent. */
function oracleParty(seed: string): ParticipantInfo {
  return {
    address: seededAddress(seed),
    lockHash: `0x${seed.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
    sent: true,
    ckbDelta: '-30000',
    usedDelta: '0',
    itemDeltas: [],
  };
}

/** A .bit time oracle's update: its type scripts (args by oracle) and its one party. */
function oracle(n: number, seed: string, args: string, secondsAgo: number): GlobalActivity {
  const call = (name: string, code: string) => ({
    typeCodeHash: `0x${code.repeat(32)}`,
    typeHashType: 'type',
    typeArgs: args,
    scriptHash: `0x${code.repeat(31)}${args.slice(2)}`,
    scriptName: name,
  });
  return activityRead({
    txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
    blockNumber: 21047791 - n,
    txIndex: 1,
    timestamp: String(NOW - secondsAgo * 1000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [call('.bit Time Index State', '3a'), call('.bit Time Info', '9e')],
    lockCalls: [],
    recognizedTypes: [],
    participants: [oracleParty(seed)],
  });
}

const transfer: GlobalActivity = activityRead({
  txHash: `0x${'99'.repeat(32)}`,
  blockNumber: 21047700,
  txIndex: 2,
  timestamp: String(NOW - 600_000),
  isCellbase: false,
  protocolActions: [],
  typeCalls: [],
  lockCalls: [],
  recognizedTypes: [],
  participants: [
    { ...oracleParty('sendr1'), ckbDelta: '-100000000' },
    { ...oracleParty('recvr1'), ckbDelta: '99970000', sent: false },
  ],
});

const page = (data: GlobalActivity[]) =>
  pagedRead({ data, limit: 50, hasMore: false, nextCursor: null });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(NOW);
  vi.stubGlobal('IntersectionObserver', NoopObserver);
  window.scrollTo = vi.fn();
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the activities feed folds a run of rows that say the same thing (B4 item 5)', () => {
  // the same oracle three times running, then a transfer
  const run = [
    oracle(1, 'q3q7ue', '0x00', 49),
    oracle(2, 'q3q7ue', '0x00', 98),
    oracle(3, 'q3q7ue', '0x00', 147),
    transfer,
  ];

  // re-pinned on purpose (Gate B's rule, lane P3): the folded row named its rows' one party, the
  // same on every row by the old rule; now it counts the parties and names none
  it('draws the run as one row: its title and the code it called, its counts and its newest time', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(run));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).not.toBeNull();
    expect(folded).toHaveAttribute('data-run-count', '3');
    const head = folded.querySelector('[data-role="row-head"]') as HTMLElement;
    expect(head).toHaveTextContent(/^Script call · \.bit Time Index State · \.bit Time Info/);
    // the code by name: no row's args in a name's hover (the rows' args may differ)
    for (const name of ['.bit Time Index State', '.bit Time Info'])
      expect(within(head).getByRole('link', { name })).not.toHaveAttribute('title');
    // the newest time, the first row's (a feed is newest first), said as the newest
    expect(folded.querySelector('[data-role="evidence"]')).toHaveTextContent(
      `newest ${formatTimeAgo(run[0].timestamp, NOW)}`
    );
    // how many transactions and how many participants, counted; no party named, no amount and no
    // total: nothing is summed
    expect(folded.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^all 3 transactions ↓ · 1 sender$/
    );
    expect(folded).not.toHaveTextContent('ckb1');
    expect(folded.querySelector('[data-ckb-amount]')).toBeNull();
    expect(folded).not.toHaveTextContent('CKB');
    // the control that opens it, a real button, saying how many rows it holds
    const toggle = within(folded).getByRole('button', { name: /^all 3\stransactions ↓$/ });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    // the transfer after it is an ordinary row; the run's rows are not drawn while it is folded
    const rows = document.querySelectorAll('article[data-feed-row]');
    expect([...rows].map((r) => r.getAttribute('data-feed-row'))).toEqual([transfer.txHash]);
  });

  it('opens in place to the ordinary rows, by a click or the keyboard, and folds again', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(run));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const toggle = screen.getByRole('button', { name: /^all 3\stransactions ↓$/ });
    await user.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(toggle).toHaveTextContent('fewer ↑');
    // open, the counts stay, the transactions said as a fact after the toggle
    expect(document.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^fewer ↑ · 3 transactions · 1 sender$/
    );
    const rows = () =>
      [...document.querySelectorAll('article[data-feed-row]')].map((r) =>
        r.getAttribute('data-feed-row')
      );
    // the ordinary rows, in their place: after the run's head, before the transfer
    expect(rows()).toEqual(run.map((a) => a.txHash));
    // each with its own amount, unchanged
    expect(
      document.querySelectorAll(`article[data-feed-row="${run[0].txHash}"] [data-ckb-amount]`)
    ).toHaveLength(1);
    toggle.focus();
    await user.keyboard('{Enter}');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(rows()).toEqual([transfer.txHash]);
  });

  // re-pinned on purpose (Gate B's rule, lane P3): by the old rule the oracles' turns never
  // folded (each its own args and party); one code called, they are one run
  it('folds the three oracles of mainnet that take turns: one code, three args, three parties', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(
      page([
        oracle(1, 'hqqsv05e', '0x01', 10),
        oracle(2, '0cr43nr7', '0x02', 20),
        oracle(3, '5sq3q7ue', '0x00', 30),
        oracle(4, 'hqqsv05e', '0x01', 40),
      ])
    );
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).toHaveAttribute('data-run-count', '4');
    expect(folded.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^all 4 transactions ↓ · 3 senders$/
    );
    expect(document.querySelectorAll('article[data-feed-row]')).toHaveLength(0);
  });

  it('never folds across a row that called other code', async () => {
    const spv: GlobalActivity = {
      ...oracle(2, 'js6cy70d', '0x3d8f', 20),
      typeCalls: [
        {
          typeCodeHash: `0x${'a7'.repeat(32)}`,
          typeHashType: 'type',
          typeArgs: '0x3d8f',
          scriptHash: `0x${'d0'.repeat(32)}`,
          scriptName: 'Bitcoin SPV Type Lock',
        },
      ],
    };
    const rows = [oracle(1, 'hqqsv05e', '0x01', 10), spv, oracle(3, '0cr43nr7', '0x02', 30)];
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(rows));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    expect(document.querySelector('[data-role="activity-run"]')).toBeNull();
    expect(
      [...document.querySelectorAll('article[data-feed-row]')].map((r) =>
        r.getAttribute('data-feed-row')
      )
    ).toEqual(rows.map((a) => a.txHash));
  });

  // The live feeds (re-captured for the P3 follow-up through this lane's API build over the live
  // mainnet stores on 2026-11-19, with the item types of then, and again at the third read's head,
  // 574cb27d, T NOTE-7; the capture is said in the fixture, its item types read as the recognised
  // codes, `liveFeeds` says how): the first page of /activities, and the token feed's sUDT
  // transfers, whose runs name their standard.
  // The live capture (2026-11-19) predates the senders bit: its parties do not say whether they
  // sent. Until it is re-captured over a re-synced store, these tests mark as senders the parties
  // whose CKB fell: such a party's inputs carried more than its outputs, so it held an input
  // (exact that way round; a sender whose CKB did not fall is not marked, so the counts pinned
  // below are of those parties).
  const feeds = Object.fromEntries(
    Object.entries(liveFeeds()).map(([feed, rows]) => [
      feed,
      rows.map((a) => {
        // the parties whose CKB fell, by the short id the route names them by: the senders bit
        // marked in their tags (the fold counts it there, lane feed); the parties a row of more
        // than six does not draw carry no CKB change here, so none of them is marked
        const fell = new Set(
          a.participants
            .filter((p) => BigInt(p.ckbDelta) < BigInt(0))
            .map((p) => (p.lockHash ?? p.lockHashPrefix ?? '').slice(0, 42))
        );
        return activityRead({
          ...a,
          participants: a.participants.map((p) => ({
            ...p,
            sent: BigInt(p.ckbDelta) < BigInt(0),
          })),
          partyIds: a.partyIds.map(
            ([id, tags]): PartyId => [id, fell.has(id) ? tags | TAG_INPUT : tags]
          ),
        });
      }),
    ])
  );
  const said = (el: Element) =>
    el.getAttribute('data-role') === 'activity-run'
      ? `${el.querySelector('[data-role="head-main"]')!.textContent!.replace(/\s+/g, ' ')} | ${el.querySelector('[data-role="run-count"]')!.textContent!.replace(/\s+/g, ' ')}`
      : el.querySelector('[data-role="headline"]')!.textContent;
  const drawnFeed = async (rows: GlobalActivity[]) => {
    vi.setSystemTime(Number(rows[0].timestamp) + 60_000);
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(rows));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    return [...document.querySelectorAll('[data-role="activity-run"], article[data-feed-row]')].map(
      said
    );
  };

  // re-pinned on purpose at the recapture (T NOTE-7): the whole feed is the chain's newest page,
  // #21,057,151 to #21,057,212; each run's rows and parties counted apart in Python from the
  // fixture's code sets and lock hashes agree with every line here. Re-pinned on purpose
  // (resync/int, at the merge into resync/type-codes): each run counts its senders (here the
  // parties whose CKB fell, the capture's one evidence of an input), where it counted every party;
  // the same 12 runs, their senders counted apart in Python from the fixture's lock hashes and CKB
  // changes (2, 3, 3, 2, 3, 2, 3, 5, 3, 3, 3 for the eleven runs)
  it('draws the live feed’s first page, 50 activities, in its runs', async () => {
    expect(await drawnFeed(feeds.all)).toEqual([
      'Script call · Bitcoin SPV Type Lock | all 2 transactions ↓ · 2 senders',
      'Script call · .bit Time Index State · .bit Time Info | all 3 transactions ↓ · 3 senders',
      'CKB Transfer',
      'Script call · .bit Time Index State · .bit Time Info | all 3 transactions ↓ · 3 senders',
      'CKB Transfer | all 2 transactions ↓ · 2 senders',
      'Script call · .bit Time Index State · .bit Time Info | all 6 transactions ↓ · 3 senders',
      'Script call · Bitcoin SPV Type Lock | all 2 transactions ↓ · 2 senders',
      'Script call · .bit Time Index State · .bit Time Info | all 6 transactions ↓ · 3 senders',
      'CKB Transfer | all 5 transactions ↓ · 5 senders',
      'Script call · .bit Time Index State · .bit Time Info | all 12 transactions ↓ · 3 senders',
      'CKB Transfer | all 2 transactions ↓ · 3 senders',
      'Script call · .bit Time Index State · .bit Time Info | all 6 transactions ↓ · 3 senders',
    ]);
  });

  // The lead's final round, item 1: the code is a set, so the 29 transfers that called the same
  // codes with two to five Godwoken Withdrawal Lock outputs are one run, and its head names each
  // code once: no "×2", which is a row's count of its own calls, not the run's
  it('folds the live token transfers into one run whose head names each code once', async () => {
    const drawn = await drawnFeed(feeds.token);
    expect(drawn).toHaveLength(1);
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).toHaveAttribute('data-run-count', '29');
    expect(folded.querySelector('[data-role="head-main"]')!.textContent).not.toMatch(/×/);
  });

  // the page-phase review's MUST-1: a folded run names the code its rows' items carry, here the
  // standard of the tokens they moved ("Simple UDT"), whatever tokens each row moved
  it('names the standard a run of token transfers carries, its tokens apart', async () => {
    // re-pinned on purpose (the lead's final round, item 1): four runs, [11, 11, 3, 4], cut only by
    // the count of Godwoken Withdrawal Lock outputs ("×2" to "×5" on the head), are one
    expect(await drawnFeed(feeds.token)).toEqual([
      // re-pinned on purpose (the lead's final round, item 2): a folded head names its item types
      // first and six names at most, as a row's head does; the eight were "… · lock
      // 0xede19974…56f0ba · Godwoken Withdrawal Lock · lock 0xff022d55…d32733". Re-pinned on
      // purpose (resync/int): the run counts its senders (the parties whose CKB fell), where it
      // counted 149 participants; re-pinned on purpose (ruling 6 of gate P): the run counts its
      // tokens too, the six sets' five (USDC, USDC|bsc, dCKB, WBTC, BTCB|bsc), each by its type
      // script. Re-pinned on purpose at the merge of lane feed: the senders are counted from the
      // senders bit of the route's party ids, which this capture marks on the parties whose CKB
      // fell; its rows (8 to 11 parties each) now draw five parties, so only theirs can be marked:
      // 17 of the run's 149 parties (31 where every party was sent in full; both counted apart in
      // Python from the two captures), until the capture is re-taken over a re-synced store
      'Token transfer · Simple UDT · Type ID · 0xa9267ff5…00e981 · lock 0x1c04df09…0ee5ac · Godwoken Custodian Lock +3 more locks | all 29 transactions ↓ · 17 senders · 5 tokens',
    ]);
  });

  // Lane B4 fix round (review-rules-logic NOTE 1, the coordinator's item 11): a row whose
  // protocol action cannot be read reports it in its place, so it is never folded: folded, its
  // error showed only when the run was opened. Three deposits by one party, the middle one's
  // metadata unreadable: it stands alone, so no two of them are consecutive and nothing folds.
  it('never folds a row whose protocol action cannot be read: its error stands in its place', async () => {
    const deposit = (n: number, metadata: Record<string, string>): GlobalActivity =>
      activityRead({
        ...oracle(n, 'q3q7ue', '0x00', n * 49),
        recognizedTypes: [ITEM_TYPE.dao],
        typeCalls: [],
        protocolActions: [
          { protocol: 'dao', protocolName: 'Nervos DAO', action: 'deposit', metadata },
        ],
      });
    const rows = [
      deposit(1, { capacity: '10200000000' }),
      deposit(2, {}),
      deposit(3, { capacity: '10200000000' }),
    ];
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(rows));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    expect(document.querySelector('[data-role="activity-run"]')).toBeNull();
    expect(
      [...document.querySelectorAll('article[data-feed-row]')].map((r) =>
        r.getAttribute('data-feed-row')
      )
    ).toEqual(rows.map((a) => a.txHash));
    expect(screen.getByRole('alert')).toHaveTextContent(`Cannot read tx ${rows[1].txHash}`);
  });

  it('never folds across a day: a run ends at the day’s divider', async () => {
    const day = 86_400;
    vi.mocked(api.getGlobalActivities).mockResolvedValue(
      page([
        oracle(1, 'q3q7ue', '0x00', 60),
        oracle(2, 'q3q7ue', '0x00', 120),
        // yesterday
        oracle(3, 'q3q7ue', '0x00', day + 60),
        oracle(4, 'q3q7ue', '0x00', day + 120),
      ])
    );
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const runs = [...document.querySelectorAll('[data-role="activity-run"]')];
    expect(runs.map((r) => r.getAttribute('data-run-count'))).toEqual(['2', '2']);
  });
});

// C7 (resync/type-codes): the API states on every row the recognised type script codes that ran
// (`recognizedTypes`, read from the row), where it stated the items' codes or why it could not
// (`itemTypes`, `itemTypesUnread`). A wallet merging its own sUDT cells moves no item: it read as
// a CKB transfer with no code, and folded with the plain transfers around it. Its code is on the
// row now and its party carries TAG_TYPE_CALL: it is a script call that names the code that ran.
describe('a row’s recognised codes (C7)', () => {
  const SUDT = {
    typeCodeHash: '0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5',
    typeHashType: 'type',
    scriptName: 'Simple UDT',
  };
  /** A wallet merging its own sUDT cells: the code ran, no token changed hands. */
  const merge = (n: number): GlobalActivity =>
    activityRead({
      ...transfer,
      txHash: `0x${String(n).repeat(32)}`,
      blockNumber: 21047700 - n,
      timestamp: String(NOW - 600_000 - n * 1000),
      recognizedTypes: [SUDT],
      participants: [{ ...oracleParty('mergr1'), tags: TAG_TYPE_CALL }],
    });
  const plain = (n: number): GlobalActivity => ({
    ...transfer,
    txHash: `0x${String(n).repeat(32)}`,
    blockNumber: 21047700 - n,
    timestamp: String(NOW - 600_000 - n * 1000),
  });
  const said = (el: Element) =>
    el.getAttribute('data-role') === 'activity-run'
      ? `${el.querySelector('[data-role="head-main"]')!.textContent!.replace(/\s+/g, ' ')} | ${el.querySelector('[data-role="run-count"]')!.textContent!.replace(/\s+/g, ' ')}`
      : el.querySelector('[data-role="head-main"]')!.textContent!.replace(/\s+/g, ' ');
  const drawnFeed = async (rows: GlobalActivity[]) => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(rows));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    return [...document.querySelectorAll('[data-role="activity-run"], article[data-feed-row]')].map(
      said
    );
  };

  it('never folds a wallet’s sUDT merge with the plain CKB transfers around it, and names its code', async () => {
    expect(await drawnFeed([plain(11), merge(12), plain(13)])).toEqual([
      'CKB Transfer',
      'Script call · Simple UDT',
      'CKB Transfer',
    ]);
  });

  it('folds merges of one code into a run whose head names the code that ran', async () => {
    expect(await drawnFeed([plain(11), merge(12), merge(13), merge(14)])).toEqual([
      'CKB Transfer',
      'Script call · Simple UDT | all 3 transactions ↓ · 1 sender',
    ]);
  });

  // At the merge of resync/all: a recognised code is named by the code its pair runs, and where
  // the pair is not the registry's reference for that code the API writes its hash type beside the
  // name (`scriptNameBy`), which a head writes by the one text rule (`withNameBy`, lib/lock-words)
  it('names a recognised code run by another reference than the registry’s with its hash type', async () => {
    const byData2 = (n: number): GlobalActivity =>
      activityRead({
        ...merge(n),
        recognizedTypes: [
          {
            typeCodeHash: '0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
            typeHashType: 'data2',
            scriptName: 'xUDT',
            scriptNameBy: 'data2',
          },
        ],
      });
    expect(await drawnFeed([plain(11), byData2(12), plain(13)])).toEqual([
      'CKB Transfer',
      'Script call · xUDT · by data2',
      'CKB Transfer',
    ]);
  });

  // runSubject's refusals: below, beside the item-types ruling's three branches they replace
  it('gives every row its subject by the codes it states, a committed row and a pool row alike', () => {
    expect(runSubject(merge(12), 'Script call')?.recognizedTypes).toEqual([SUDT]);
    expect(runSubject(plain(11), 'CKB Transfer')?.recognizedTypes).toEqual([]);
    const pending = { ...merge(12), blockNumber: null, timestamp: null };
    expect(runSubject(pending, 'Script call')?.recognizedTypes).toEqual([SUDT]);
  });
});

// Lane B4 fix round (review-rules-logic MUST 2 and SHOULD 1, tests NOTE-5; the coordinator's item
// 3): a live feed adds rows above (a poll near the top) and below (an older page), and neither
// remounts what the reader has in front of them. A day's rows are keyed by the day, a row by its
// transaction, a run's head by the first row it was seen with; a run is open while any of its rows
// is one the reader opened or saw drawn whole, so a row arriving above or below an open run
// joins it open, and a row drawn whole never folds under the reader.
describe('a live feed keeps what the reader opened and where their focus is (B4 fix round)', () => {
  const run = [
    oracle(1, 'q3q7ue', '0x00', 49),
    oracle(2, 'q3q7ue', '0x00', 98),
    oracle(3, 'q3q7ue', '0x00', 147),
    transfer,
  ];
  /** A transfer between seven parties: its row folds two of them under "+2 participants". */
  const crowd: GlobalActivity = activityRead({
    ...transfer,
    txHash: `0x${'77'.repeat(32)}`,
    blockNumber: 21047699,
    timestamp: String(NOW - 700_000),
    participants: ['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((c, i) => ({
      ...oracleParty(`crowd${c}`),
      ckbDelta: String((i + 1) * 100_000_000),
    })),
  });
  /** A row newer than all of them, saying something else. */
  const fresh: GlobalActivity = {
    ...transfer,
    txHash: `0x${'f0'.repeat(32)}`,
    blockNumber: 21047795,
    timestamp: String(NOW - 5_000),
  };

  function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((r) => (resolve = r));
    return { promise, resolve };
  }

  /** The stream's first page, then the head poll, answered when the test says. */
  function stream(first: GlobalActivity[]) {
    const head = deferred<ReturnType<typeof page>>();
    let calls = 0;
    vi.mocked(api.getGlobalActivities).mockImplementation(() =>
      calls++ === 0 ? Promise.resolve(page(first)) : head.promise
    );
    return head;
  }

  const feedRows = () =>
    [...document.querySelectorAll('article[data-feed-row]')].map((r) =>
      r.getAttribute('data-feed-row')
    );

  it('a new row at the top leaves an opened run open, its toggle the same node, with the focus', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    const head = stream(run);
    render(<ActivitiesStreamExplorer />);
    const toggle = await screen.findByRole('button', { name: /^all 3\stransactions ↓$/ });
    await user.click(toggle);
    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    head.resolve(page([fresh, ...run]));
    await waitFor(() => expect(feedRows()[0]).toBe(fresh.txHash));
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(document.activeElement).toBe(toggle);
    expect(feedRows()).toEqual([fresh.txHash, ...run.map((a) => a.txHash)]);
  });

  // B4 third round (review-rules-fixes2 MUST 1, the coordinator's item 1): block times may step
  // backwards across midnight UTC+8, so a new row at the top can carry a date that already stands
  // lower in the stream. Its group is new; the lower group of that date, where the reader opened
  // a run and focused its toggle, keeps its key and every row in it.
  it('a new row whose date already stands lower leaves that lower group as it was', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    // NOW is 16:00 UTC+8 on Nov 18: a row a minute ago is Nov 18, one 20 hours ago Nov 17
    const today = { ...transfer, txHash: `0x${'d1'.repeat(32)}`, timestamp: String(NOW - 60_000) };
    const yesterday = [oracle(1, 'q3q7ue', '0x00', 72_000), oracle(2, 'q3q7ue', '0x00', 72_100)];
    const head = stream([today, ...yesterday]);
    render(<ActivitiesStreamExplorer />);
    const toggle = await screen.findByRole('button', { name: /^all 2\stransactions ↓$/ });
    await user.click(toggle);
    toggle.focus();
    // a newer block whose time is Nov 17, above the Nov 18 row
    const back = {
      ...transfer,
      txHash: `0x${'d2'.repeat(32)}`,
      blockNumber: 21047799,
      timestamp: String(NOW - 71_000_000),
    };
    head.resolve(page([back, today, ...yesterday]));
    await waitFor(() => expect(feedRows()[0]).toBe(back.txHash));
    expect(
      [...document.querySelectorAll('[data-testid^="activity-day-divider"]')].map(
        (d) => d.textContent
      )
    ).toEqual(['Yesterday', 'Today', 'Yesterday']);
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(document.activeElement).toBe(toggle);
  });

  it('a new row at the top leaves a row’s own state alone: its participants stay opened', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    const head = stream([crowd]);
    render(<ActivitiesStreamExplorer />);
    // re-pinned on purpose (the lead's final round, item 3): the participants' fold is
    // FoldToggle's box under their lines, "all 7 participants ↓" / "fewer ↑"
    const more = await screen.findByRole('button', { name: /^all 7\sparticipants ↓$/ });
    await user.click(more);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    const article = document.querySelector(`article[data-feed-row="${crowd.txHash}"]`)!;
    head.resolve(page([fresh, crowd]));
    await waitFor(() => expect(feedRows()).toEqual([fresh.txHash, crowd.txHash]));
    expect(article.isConnected).toBe(true);
    expect(more.isConnected).toBe(true);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    expect(within(article as HTMLElement).getByRole('button', { name: 'fewer ↑' })).toBe(more);
  });

  it('a new row that says the same as an open run joins it, open, its head and focus kept', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    const head = stream(run);
    render(<ActivitiesStreamExplorer />);
    const toggle = await screen.findByRole('button', { name: /^all 3\stransactions ↓$/ });
    await user.click(toggle);
    toggle.focus();
    const same = oracle(0, 'q3q7ue', '0x00', 5);
    head.resolve(page([same, ...run]));
    const runCount = () =>
      document.querySelector('[data-role="activity-run"]')?.getAttribute('data-run-count');
    await waitFor(() => expect(runCount()).toBe('4'));
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(document.activeElement).toBe(toggle);
    expect(feedRows()).toEqual([same, ...run].map((a) => a.txHash));
  });
});

// Lane B4 second fix round (review-rules-fixes-logic NOTE 2, the coordinator's item 9): a filter
// change shows a new set of rows, and the runs' state starts anew. On All two oracle updates
// stand apart, each drawn whole; under Script calls they stand together and fold. Back on a
// filter read before, its rows come from the cache with no loading in between, and the run must
// still be drawn folded: the reader never opened it.
describe('a filter change starts the runs anew (B4 second fix round)', () => {
  it('folds a run the reader never opened, though its rows were drawn whole under another filter', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    const a = oracle(1, 'q3q7ue', '0x00', 49);
    const b = oracle(2, 'q3q7ue', '0x00', 98);
    vi.mocked(api.getGlobalActivities).mockImplementation(async (params) =>
      page(params?.filter === 'script' ? [a, b] : [a, transfer, b])
    );
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    // the kind filter's radios (controls lane C2, re-pinned on purpose: they were the
    // aria-pressed buttons of the group "Filter by kind", "Script calls" and "All")
    const chip = (name: RegExp) =>
      within(screen.getByRole('radiogroup', { name: 'kind' })).getByRole('radio', { name });
    const runShown = () =>
      waitFor(() => expect(document.querySelector('[data-role="activity-run"]')).not.toBeNull());
    // Script calls (read), All (read before: from the cache), Script calls again (from the cache)
    await user.click(chip(/^script calls/));
    await runShown();
    await user.click(chip(/^all/));
    await waitFor(() => expect(document.querySelector('[data-role="activity-run"]')).toBeNull());
    expect(document.querySelectorAll('article[data-feed-row]')).toHaveLength(3);
    await user.click(chip(/^script calls/));
    await runShown();
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(within(folded).getByRole('button')).toHaveAttribute('aria-expanded', 'false');
    expect(document.querySelectorAll('article[data-feed-row]')).toHaveLength(0);
  });
});

describe('an older page that continues the last run (B4 fix round)', () => {
  /** The stream's first page (more after it), then the older page; the head poll finds nothing. */
  function pages(first: GlobalActivity[], older: GlobalActivity[]) {
    vi.mocked(api.getGlobalActivities).mockImplementation(async (params) =>
      params?.cursor === 'older'
        ? pagedRead({ data: older, limit: 50, hasMore: false, nextCursor: null })
        : // a full page of its rows with more after it (lane cd-fixes-controls, review-p-tests
          // SHOULD-5: it was the first rows on a page of fifty that said more followed); the
          // stream's next button says that page's size, so the clicks below find it by its form
          pagedRead({ data: first, limit: first.length, hasMore: true, nextCursor: 'older' })
    );
  }
  const feedRows = () =>
    [...document.querySelectorAll('article[data-feed-row]')].map((r) =>
      r.getAttribute('data-feed-row')
    );

  it('draws a row the reader sees whole still whole: the run it starts is drawn open', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    const last = oracle(4, 'q3q7ue', '0x00', 200);
    const next = oracle(5, 'q3q7ue', '0x00', 250);
    pages([transfer, last], [next]);
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    expect(document.querySelector('[data-role="activity-run"]')).toBeNull();
    const drawn = document.querySelector(`article[data-feed-row="${last.txHash}"]`)!;
    await user.click(screen.getByRole('button', { name: /^next \d+ ↓$/ }));
    await waitFor(() => expect(feedRows()).toContain(transfer.txHash));
    await waitFor(() =>
      expect(document.querySelector('[data-role="activity-run"]')).toHaveAttribute(
        'data-run-count',
        '2'
      )
    );
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    const toggle = within(folded).getByRole('button');
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(drawn.isConnected).toBe(true);
    expect(feedRows()).toEqual([transfer.txHash, last.txHash, next.txHash]);
  });

  it('a folded run takes the page’s rows in and stays folded, its head the same node', async () => {
    const user = userEvent.setup({ advanceTimers: () => {} });
    const first = [transfer, oracle(4, 'q3q7ue', '0x00', 200), oracle(5, 'q3q7ue', '0x00', 250)];
    pages(first, [oracle(6, 'q3q7ue', '0x00', 300)]);
    render(<ActivitiesStreamExplorer />);
    // re-pinned on purpose (the lead's finding, the exactness lane's B9): the run cut by the
    // page's end, older rows unread, is not final ("2 transactions so far"); with the older page
    // in, the feed's end read, it is all 3 (it said "all 2" over the unread rows)
    const toggle = await screen.findByRole('button', { name: /^2\stransactions so far ↓$/ });
    // re-pinned on purpose (resync/int, at the merge into resync/type-codes): the run counts its
    // senders, so far too
    expect(document.querySelector('[data-role="run-count"]')!.textContent).toMatch(
      /senders? so far$/
    );
    await user.click(screen.getByRole('button', { name: /^next \d+ ↓$/ }));
    await waitFor(() => expect(toggle).toHaveTextContent(/^all 3\stransactions ↓$/));
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(feedRows()).toEqual([transfer.txHash]);
  });
});

// Lane B4 fix round (review-rules-logic NOTE 2) showed a folded run's parties by the rows' rule;
// re-pinned on purpose (Gate B's rule, lane P3): a run's rows name other parties from row to row,
// so the folded row names none and counts them, each once. Re-pinned on purpose (resync/int, the
// owner's word at Gate B): it counts the parties that SENT, each once (it counted all 12). A run of
// 7-party transactions, two parties in common, the first three of each the senders.
describe('a folded run counts its senders and names no party (Gate B)', () => {
  it('counts every party that sent, once, and draws no party line', async () => {
    const user = userEvent.setup();
    const crowded = (n: number, seeds: string[]): GlobalActivity =>
      activityRead({
        ...oracle(n, 'q3q7ue', '0x00', n * 49),
        participants: seeds.map((seed, i) => ({
          ...oracleParty(seed),
          ckbDelta: String(i - 3),
          sent: i < 3,
        })),
      });
    const rows = [
      crowded(1, ['c0', 'c1', 'c2', 'c3', 'c4', 'c5', 'c6']),
      crowded(2, ['c0', 'c1', 'd2', 'd3', 'd4', 'd5', 'd6']),
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
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded.querySelector('[data-role="run-count"]')).toHaveTextContent(
      // c0, c1, c2 sent in the first; c0, c1, d2 in the second
      /^all 2 transactions ↓ · 4 senders$/
    );
    expect(folded.querySelectorAll('[data-role="party"]')).toHaveLength(0);
    await user.click(within(folded).getByRole('button', { name: /^all 2\stransactions ↓$/ }));
    // opened, each row draws its own parties by the rows' rule (over six, five and the count)
    const row = document.querySelector(`article[data-feed-row="${rows[1].txHash}"]`)!;
    expect(row).toHaveTextContent(/all 7\sparticipants ↓/);
  });
});

// resync/int, step 2: a cellbase has no input, so no party of it sent: a run of cellbases folds with
// no count of senders; a committed run of transactions no party of which sent cannot be (every
// transaction but a cellbase has an input), so it is refused in its place, never "0 senders".
describe('senders and cellbases (resync/int)', () => {
  const drawn = (rows: GlobalActivity[]) =>
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

  it('folds a run of cellbases with no count of senders', () => {
    const cellbase = (n: number): GlobalActivity =>
      activityRead({
        ...oracle(n, 'm1ner0', '0x00', n * 8),
        isCellbase: true,
        typeCalls: [],
        participants: [{ ...oracleParty('m1ner0'), ckbDelta: '100000000000', sent: false }],
      });
    drawn([cellbase(1), cellbase(2), cellbase(3)]);
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).toHaveAttribute('data-run-count', '3');
    expect(folded.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^all 3 transactions ↓$/
    );
  });

  it('refuses in its place a run of transactions no party of which sent', () => {
    const silent = (n: number): GlobalActivity =>
      activityRead({
        ...oracle(n, 'q3q7ue', '0x00', n * 49),
        participants: [{ ...oracleParty('q3q7ue'), sent: false }],
      });
    vi.spyOn(console, 'error').mockImplementation(() => {});
    drawn([silent(1), silent(2)]);
    expect(document.querySelector('[data-role="activity-run"]')).toBeNull();
    expect(screen.getByTestId('activity-read-error')).toHaveTextContent(
      `Cannot read tx ${silent(1).txHash}: transaction ${silent(1).txHash}: a committed transaction with no party that sent (every transaction but a cellbase has an input)`
    );
  });
});

// Lane B4 fix round (review-rules-logic NOTE 8, the coordinator's item 16): the run's toggle is
// a real button, and on a phone it is a target a finger can take. docs/RESPONSIVE_LAYOUT.md's
// Touch rule names no size; the size is WCAG 2.2's minimum target (2.5.8), 24 × 24 CSS px, the
// figure the review measured against and the site's own (the hardfork timeline's DOT_HIT_PX).
// The toggle stands at least 24 px tall at every width; it stood on its 20 px row line.
describe('the run’s toggle on a phone (B4 fix round)', () => {
  const theme = resolveConfig(tailwindConfig).theme as unknown as {
    height: Record<string, string>;
    minHeight: Record<string, string>;
    lineHeight: Record<string, string>;
  };
  const px = (v: string) => (v.endsWith('rem') ? parseFloat(v) * 16 : parseFloat(v));
  /**
   * The height of a button that sets its line, its minimum height or its height in classes and
   * nothing else vertical (refused otherwise), at a viewport: the largest of them. Re-pinned on
   * purpose (lists lane P3): the toggle is the site's one fold control, `FoldToggle`, which sets its
   * height (32 px below 640, 28 from it) and no line.
   */
  function targetPx(el: Element, viewport: number): number {
    const at = (prefix: string) => {
      let value: string | undefined;
      let level = -1;
      for (const cls of el.classList) {
        const parts = cls.split(':');
        const utility = parts.pop()!;
        if (!utility.startsWith(prefix)) continue;
        if (parts.length > 1) throw new Error(`targetPx: ${cls} is not read`);
        const screen = parts[0];
        const i = screen === undefined ? 0 : SCREENS.findIndex(([n]) => n === screen) + 1;
        if (i < 0) throw new Error(`targetPx: ${cls} is not read`);
        if (i > 0 && SCREENS[i - 1][1] > viewport) continue;
        if (i >= level) {
          level = i;
          value = utility.slice(prefix.length);
        }
      }
      return value;
    };
    for (const cls of el.classList)
      if (/^(\w+:)?(py|pt|pb|my|border-y|border-t|border-b|max-h)-/.test(cls))
        throw new Error(`targetPx: ${cls} sets a height this does not read`);
    const line = at('leading-');
    const min = at('min-h-');
    const height = at('h-');
    if (line === undefined && height === undefined)
      throw new Error('targetPx: the button sets no line and no height');
    return Math.max(
      line === undefined ? 0 : px(theme.lineHeight[line]),
      min === undefined ? 0 : px(theme.minHeight[min]),
      height === undefined ? 0 : px(theme.height[height])
    );
  }

  // B4 second fix round (review-rules-fixes-logic NOTE 7, the coordinator's item 13): at every
  // width (a touch tablet stands at 768 to 1023; WCAG 2.5.8 has no breakpoint). It was 24 below md
  // and the row's 20 px line from md.
  it.each([390, 360, 768, 1024, 1280])('is at least 24 px tall at %i px', (viewport) => {
    const rows = [oracle(1, 'q3q7ue', '0x00', 49), oracle(2, 'q3q7ue', '0x00', 98)];
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
    const toggle = screen.getByRole('button', { name: /^all 2\stransactions ↓$/ });
    expect(targetPx(toggle, viewport)).toBeGreaterThanOrEqual(24);
    // above its row's line
    expect(ROW_LINE_PX).toBeLessThan(24);
  });
});

describe('a token page’s feed folds within the page it shows (B4 item 5)', () => {
  it('folds the run it loaded, and folds nothing it did not', async () => {
    const run = [oracle(1, 'q3q7ue', '0x00', 49), oracle(2, 'q3q7ue', '0x00', 98)];
    render(
      <AssetFeed
        queryKey="runs-token"
        scope="Every transaction that moved RGB++"
        plain
        mark="tokens"
        loadActivities={async () => ({ ...page(run), total: 2 })}
        loadTransactions={async () =>
          pagedRead({
            data: [],
            total: 0,
            limit: 50,
            hasMore: false,
            nextCursor: null,
          })
        }
      />
    );
    const folded = await screen.findByRole('button', { name: /^all 2\stransactions ↓$/ });
    expect(folded.closest('[data-role="activity-run"]')).toHaveAttribute('data-run-count', '2');
  });
});

describe('a run is set back as its rows are (R4)', () => {
  it('takes the class its feed gives its rows: a neighbour’s run is quiet, every word dim', () => {
    const rows = [oracle(1, 'q3q7ue', '0x00', 49), oracle(2, 'q3q7ue', '0x00', 98)];
    render(
      <ActivityRuns
        more={false}
        rows={rows.map((a) => ({
          key: a.txHash,
          subject: runSubject(a, activityTitleText(a)),
          activity: a,
          title: <ActivityTitle activity={a} />,
          className: 'quiet',
          row: <ActivityRow activity={a} className="quiet" />,
        }))}
      />
    );
    expect(document.querySelector('[data-role="activity-run"]')).toHaveClass('quiet');
  });

  // Lane B4 fix round (review-rules-tests SHOULD-2, the coordinator's item 8): the feed's own
  // decision, through the feed. An identity's feed with two of a neighbour's rows and then one of
  // its own, all three saying ".cell · ring pointer changed" with the same scripts and party: the
  // neighbour's two fold into one run, set back as their rows are, and the identity's own row is
  // not in it (its rows are compared apart from a neighbour's).
  it('folds a neighbour’s rows in an identity’s feed apart from its own, set back', async () => {
    const relinked = (n: number, actedOn: 'self' | 'neighbour'): ItemActivity => ({
      ...oracle(n, 'q3q7ue', '0x00', n * 49),
      itemContext: {
        itemId: `0x${'a7'.repeat(20)}`,
        action: 'update',
        protocolActions: ['ring_relink'],
        ringRelink: null,
        actedOn,
        createdCell: null,
        consumedCell: null,
      },
    });
    const rows = [relinked(1, 'neighbour'), relinked(2, 'neighbour'), relinked(3, 'self')];
    render(
      <AssetFeed
        queryKey="runs-neighbour"
        scope="This identity's whole life"
        item={{ kind: 'identity', protocolName: '.cell' }}
        loadActivities={async () => ({ ...page(rows), total: 3 })}
        loadTransactions={async () =>
          pagedRead({
            data: [],
            total: 0,
            limit: 50,
            hasMore: false,
            nextCursor: null,
          })
        }
      />
    );
    await waitFor(() =>
      expect(document.querySelector('[data-role="activity-run"]')).not.toBeNull()
    );
    const runs = [...document.querySelectorAll('[data-role="activity-run"]')];
    expect(runs).toHaveLength(1);
    expect(runs[0]).toHaveAttribute('data-run-count', '2');
    expect(runs[0]).toHaveClass('quiet');
    // the identity's own row, drawn alone after the run, in full ink
    const own = document.querySelector(`article[data-feed-row="${rows[2].txHash}"]`)!;
    expect(own).not.toBeNull();
    expect(own).not.toHaveClass('quiet');
    expect(
      [...document.querySelectorAll('article[data-feed-row]')].map((r) =>
        r.getAttribute('data-feed-row')
      )
    ).toEqual([rows[2].txHash]);
  });
});

// The lead's final round, item 7: a called lock is written by the one lock decision from its code
// hash, hash type and name together. A lock on the Default Lock's code hash referenced by data is
// not the Default Lock: today's API sends it the registered name ("Default Lock"), which the row
// must not write, in a row's head, a folded head or the opened run's rows, nor in a link's hover or
// address; it is written by its code hash and "by data".
describe('a called lock on a registered code hash by another hash type (the lock decision)', () => {
  const BY_DATA = {
    lockCodeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
    lockHashType: 'data',
    lockArgs: '0x' + 'ab'.repeat(20),
    scriptHash: '0x' + 'cd'.repeat(32),
    // re-keyed at the merge (lane rd/api-lock-names): the API names the code a pair runs, and a
    // lock on the default lock's code hash by data runs none, so it sends no name; the client's
    // stopgap that dropped the name is gone. A called lock's name is `lockScriptName` (the third
    // read's T SHOULD-5)
    lockScriptName: null,
    decoded: null,
    decodeError: null,
  };
  const called = (n: number): GlobalActivity => ({
    ...oracle(n, 'q3q7ue', '0x00', n * 49),
    lockCalls: [BY_DATA],
  });
  const said = (el: Element) =>
    [
      el.textContent ?? '',
      ...[...el.querySelectorAll('[title]')].map((e) => e.getAttribute('title') ?? ''),
      ...[...el.querySelectorAll('a[href]')].map((a) =>
        decodeURIComponent(a.getAttribute('href') ?? '')
      ),
    ].join('\n');

  it('writes it by its code hash and "by data", never "Default Lock", in a row and a run, opened or not', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page([called(1), called(2)]));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded.querySelector('[data-role="head-main"]')).toHaveTextContent(
      /lock 0x9bd7e06f…a3cce8 by data/
    );
    expect(said(folded)).not.toMatch(/Default Lock/);
    await userEvent
      .setup({ advanceTimers: () => {} })
      .click(within(folded).getByRole('button', { name: /^all 2\stransactions ↓$/ }));
    const rows = [...document.querySelectorAll('article[data-feed-row]')];
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.querySelector('[data-role="head-main"]')).toHaveTextContent(/by data/);
      expect(said(row)).not.toMatch(/Default Lock/);
    }
  });
});

// The third read's T MUST-2: the page's half of the item-types ruling, one case per branch of
// `runSubject`. Re-pinned on purpose at the merge into resync/type-codes (C7): the branches change
// with the field. The route reads a row's recognised codes from the row (`recognizedTypes`), never
// per item, so no committed row's codes are unread: "an unread row stands alone" is replaced by
// "every row folds by its codes"; "null with no reason" by "a list that is no list is refused";
// "no field at all" stays a refusal by the one reader's sentence. Each case was checked by hand
// against its mutation in components/activity/activity-runs.tsx `runSubject`.
describe('runSubject: a row’s recognised codes (the item-types ruling, carried to C7)', () => {
  /** A token transfer of the oracle's party: its sUDT moved, the code on the row. */
  const moved = (n: number): GlobalActivity => {
    const row = oracle(n, 'q3q7ue', '0x00', n * 49);
    return activityRead({
      ...row,
      typeCalls: [],
      participants: [
        {
          ...row.participants[0],
          itemDeltas: [{ kind: 'token', typeScriptHash: `0x${'5e'.repeat(32)}`, delta: '1' }],
          tags: TAG_TOKEN,
        },
      ],
      recognizedTypes: [ITEM_TYPE.sudt],
    });
  };

  // make runSubject stand a committed row alone (return null) as the unread branch did: the two
  // are two rows, no run: red
  it('folds every committed row by the codes it states: no row stands alone for its codes', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page([moved(1), moved(2)]));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const folded = document.querySelector('[data-role="activity-run"]');
    expect(folded).toHaveAttribute('data-run-count', '2');
  });

  // delete the refusal of a list that is no list: runKey reads `null.map`, a TypeError that names
  // no transaction: red
  it('refuses a row whose recognised codes are no list, by name', () => {
    // a row the route cannot send, on purpose: the page refuses it
    const row = unsentRow({ ...moved(1), recognizedTypes: null } as unknown as GlobalActivity);
    expect(() => runSubject(row, 'Token transfer')).toThrow(
      `transaction ${row.txHash}: the row states recognizedTypes null, not a list`
    );
  });

  // delete the one reader's refusal of a row without the field: the same TypeError: red
  it('refuses a row with no recognizedTypes field at all, by name (not this API)', () => {
    const { recognizedTypes: _gone, ...row } = moved(1);
    expect(() => runSubject(row as GlobalActivity, 'Token transfer')).toThrow(
      // the one reader's sentence (lib/api-fields.ts sentField)
      `transaction ${row.txHash}: the API sends no recognizedTypes: it is older than this page`
    );
  });
});

// The third read's Lb NOTE 9: the runs remembered every row they ever drew alone and every row's
// run head, for the feed's whole life (the shape 2e bounded for the stream's day anchors). What
// they carry to the next render is bound to the rows in the feed now.
describe('what the runs remember between renders', () => {
  const runOf = (...keys: string[]) => ({ run: keys.map((key) => ({ key })), head: null });
  const folded = (head: string, ...keys: string[]) => ({ run: keys.map((key) => ({ key })), head });

  // keep the old memory instead of the rows present (drawn = union, heads never pruned): red
  it('holds none of the rows a feed drew and dropped', () => {
    const first = Array.from({ length: 30 }, (_, i) => runOf(`a${i}`));
    let memory = runMemory([...first, folded('b0', 'b0', 'b1')], new Set());
    expect(memory.drawn.size).toBe(30);
    expect([...memory.heads.keys()]).toEqual(['b0', 'b1']);
    memory = runMemory([runOf('c0'), folded('b1', 'c1', 'c2')], memory.drawn);
    expect([...memory.drawn]).toEqual(['c0']);
    expect([...memory.heads]).toEqual([
      ['c1', 'b1'],
      ['c2', 'b1'],
    ]);
  });

  it('keeps a row drawn alone that a later row joins, so its run is drawn open', () => {
    let memory = runMemory([runOf('a0')], new Set());
    memory = runMemory([folded('a0', 'a0', 'a1')], memory.drawn);
    expect(memory.drawn.has('a0')).toBe(true);
    expect(memory.heads.get('a1')).toBe('a0');
  });
});

// The lead's finding (in Chrome, the exactness lane's B9): a folded run that reaches the last READ
// row while the feed has older rows unread said "all 59 transactions ↓ · 918 participants" over
// "Reading older activities…", and "all 64 … 1,050" once the next page was in. A count that is not
// final says so: the last run of what is read, while older rows are unread (`more`), states both
// counts as so far; when the feed's end is read, or the next run begins, it is "all N" again.
describe('a run whose count is not final (older rows unread)', () => {
  const asRows = (rows: GlobalActivity[]) =>
    rows.map((a) => ({
      key: a.txHash,
      subject: runSubject(a, activityTitleText(a)),
      activity: a,
      title: <ActivityTitle activity={a} />,
      row: <ActivityRow activity={a} />,
    }));
  const said = () =>
    [...document.querySelectorAll('[data-role="run-count"]')].map((el) =>
      el.textContent!.replace(/\s+/g, ' ')
    );

  // drop `more` (every run final): "all 2 …", red
  it('says so far of a run that ends exactly at the read rows’ end with more unread', () => {
    render(
      <ActivityRuns
        more
        rows={asRows([
          transfer,
          oracle(4, 'q3q7ue', '0x00', 200),
          oracle(5, 'q3q7ue', '0x00', 250),
        ])}
      />
    );
    // re-pinned on purpose (resync/int, at the merge into resync/type-codes): the oracle's party
    // sent, the run counts it as its one sender
    expect(said()).toEqual(['2 transactions so far ↓ · 1 sender so far']);
  });

  it('says all of a run that ends before the read rows’ last row, and of the last page’s runs', () => {
    const run = [oracle(4, 'q3q7ue', '0x00', 200), oracle(5, 'q3q7ue', '0x00', 250)];
    const first = render(<ActivityRuns more rows={asRows([...run, transfer])} />);
    // re-pinned on purpose (resync/int, at the merge into resync/type-codes): one sender
    expect(said()).toEqual(['all 2 transactions ↓ · 1 sender']);
    first.unmount();
    render(<ActivityRuns more={false} rows={asRows([transfer, ...run])} />);
    expect(said()).toEqual(['all 2 transactions ↓ · 1 sender']);
  });
});
