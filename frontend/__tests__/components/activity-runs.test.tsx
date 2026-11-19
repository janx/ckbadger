// Readability 2026-11-18, lane B4 (pG) item 5, with the rule the owner ruled at Gate B (lists lane
// P3): consecutive rows that did the same thing and called the same script code (not the args, not
// the parties) become one row that says what they called, how many transactions and how many
// participants, and the newest time, opened in place. It counts and sums nothing: the folded row
// shows no amount and no total. The rule of "the same" is lib/activity-runs.ts (its own test); this
// pins how the feeds draw a run.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { AssetFeed } from '@/components/asset/asset-feed';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import type { GlobalActivity, ItemActivity, ParticipantInfo } from '@/lib/api';
import { api } from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';
import live from '@/__tests__/fixtures/activities-live-mainnet.json';
import { ROW_LINE_PX } from '@/lib/type-metrics';
import { SCREENS } from '@/__tests__/utils/tailwind-box';
import resolveConfig from 'tailwindcss/resolveConfig';
import tailwindConfig from '@/tailwind.config';
import { pagedRead } from '@/__tests__/utils/paged-read';

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

const NOW = Date.parse('2026-11-18T08:00:00Z');

function oracleParty(seed: string): ParticipantInfo {
  return {
    address: seededAddress(seed),
    lockHash: `0x${seed.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
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
  return {
    txHash: `0x${String(n).padStart(2, '0').repeat(32)}`,
    blockNumber: 21047791 - n,
    txIndex: 1,
    timestamp: String(NOW - secondsAgo * 1000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [call('.bit Time Index State', '3a'), call('.bit Time Info', '9e')],
    lockCalls: [],
    participants: [oracleParty(seed)],
  };
}

const transfer: GlobalActivity = {
  txHash: `0x${'99'.repeat(32)}`,
  blockNumber: 21047700,
  txIndex: 2,
  timestamp: String(NOW - 600_000),
  isCellbase: false,
  protocolActions: [],
  typeCalls: [],
  lockCalls: [],
  participants: [
    { ...oracleParty('sendr1'), ckbDelta: '-100000000' },
    { ...oracleParty('recvr1'), ckbDelta: '99970000' },
  ],
};

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
      /^all 3 transactions ↓ · 1 participant$/
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
      /^fewer ↑ · 3 transactions · 1 participant$/
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
      /^all 4 transactions ↓ · 3 participants$/
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

  // The live feed (mainnet, the first page of /activities read from the API on 2026-11-19, blocks
  // #21,054,083 to #21,054,163): 50 activities drawn as 9 rows, five of them folded runs.
  it('draws the live feed’s first page, 50 activities, as 9 rows', async () => {
    const rows = live as unknown as GlobalActivity[];
    vi.setSystemTime(Number(rows[0].timestamp) + 60_000);
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(rows));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const drawn = [
      ...document.querySelectorAll('[data-role="activity-run"], article[data-feed-row]'),
    ];
    const said = (el: Element) =>
      el.getAttribute('data-role') === 'activity-run'
        ? `${el.querySelector('[data-role="head-main"]')!.textContent} | ${el.querySelector('[data-role="run-count"]')!.textContent!.replace(/\s+/g, ' ')}`
        : el.querySelector('[data-role="headline"]')!.textContent;
    expect(drawn.map(said)).toEqual([
      'Script call · .bit Time Index State · .bit Time Info | all 7 transactions ↓ · 3 participants',
      'Script call',
      'Script call · .bit Time Index State · .bit Time Info | all 2 transactions ↓ · 2 participants',
      'Script call',
      'Script call · .bit Time Index State · .bit Time Info | all 9 transactions ↓ · 3 participants',
      '.bit · recycle expired account',
      'Script call · .bit Time Index State · .bit Time Info | all 21 transactions ↓ · 3 participants',
      'CKB Transfer',
      'Script call · .bit Time Index State · .bit Time Info | all 7 transactions ↓ · 3 participants',
    ]);
  });

  // Lane B4 fix round (review-rules-logic NOTE 1, the coordinator's item 11): a row whose
  // protocol action cannot be read reports it in its place, so it is never folded: folded, its
  // error showed only when the run was opened. Three deposits by one party, the middle one's
  // metadata unreadable: it stands alone, so no two of them are consecutive and nothing folds.
  it('never folds a row whose protocol action cannot be read: its error stands in its place', async () => {
    const deposit = (n: number, metadata: Record<string, string>): GlobalActivity => ({
      ...oracle(n, 'q3q7ue', '0x00', n * 49),
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
  const crowd: GlobalActivity = {
    ...transfer,
    txHash: `0x${'77'.repeat(32)}`,
    blockNumber: 21047699,
    timestamp: String(NOW - 700_000),
    participants: ['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((c, i) => ({
      ...oracleParty(`crowd${c}`),
      ckbDelta: String((i + 1) * 100_000_000),
    })),
  };
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
    const more = await screen.findByRole('button', { name: '+2 participants' });
    await user.click(more);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    const article = document.querySelector(`article[data-feed-row="${crowd.txHash}"]`)!;
    head.resolve(page([fresh, crowd]));
    await waitFor(() => expect(feedRows()).toEqual([fresh.txHash, crowd.txHash]));
    expect(article.isConnected).toBe(true);
    expect(more.isConnected).toBe(true);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('button', { name: 'Show fewer participants' })).toBe(more);
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
    const toggle = await screen.findByRole('button', { name: /^all 2\stransactions ↓$/ });
    await user.click(screen.getByRole('button', { name: /^next \d+ ↓$/ }));
    await waitFor(() => expect(toggle).toHaveTextContent(/^all 3\stransactions ↓$/));
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(feedRows()).toEqual([transfer.txHash]);
  });
});

// Lane B4 fix round (review-rules-logic NOTE 2) showed a folded run's parties by the rows' rule;
// re-pinned on purpose (Gate B's rule, lane P3): a run's rows name other parties from row to row,
// so the folded row names none and counts them, each once. A run of 7-party transactions, two
// parties in common.
describe('a folded run counts its parties and names none (Gate B)', () => {
  it('counts every party its rows name, once, and draws no party line', async () => {
    const user = userEvent.setup();
    const crowded = (n: number, seeds: string[]): GlobalActivity => ({
      ...oracle(n, 'q3q7ue', '0x00', n * 49),
      participants: seeds.map((seed, i) => ({ ...oracleParty(seed), ckbDelta: String(i - 3) })),
    });
    const rows = [
      crowded(1, ['c0', 'c1', 'c2', 'c3', 'c4', 'c5', 'c6']),
      crowded(2, ['c0', 'c1', 'd2', 'd3', 'd4', 'd5', 'd6']),
    ];
    render(
      <ActivityRuns
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
      /^all 2 transactions ↓ · 12 participants$/
    );
    expect(folded.querySelectorAll('[data-role="party"]')).toHaveLength(0);
    await user.click(within(folded).getByRole('button', { name: /^all 2\stransactions ↓$/ }));
    // opened, each row draws its own parties by the rows' rule (over six, five and the count)
    const row = document.querySelector(`article[data-feed-row="${rows[1].txHash}"]`)!;
    expect(row).toHaveTextContent('+2 participants');
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
