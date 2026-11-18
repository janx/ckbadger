// Readability 2026-11-18, lane B4 (pG) item 5: consecutive rows with the same title, the same
// scripts and the same parties become one row with their count and the newest time, opened in
// place. It counts rows and sums nothing: the folded row shows no amount and no total. The rule of
// "the same" is lib/activity-runs.ts (its own test); this pins how the feeds draw a run.
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

const page = (data: GlobalActivity[]) => ({ data, limit: 50, hasMore: false, nextCursor: null });

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

  it('draws the run as one row: its title and scripts, its party, its count and its newest time', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page(run));
    render(<ActivitiesStreamExplorer />);
    await screen.findAllByRole('article');
    const folded = document.querySelector('[data-role="activity-run"]') as HTMLElement;
    expect(folded).not.toBeNull();
    expect(folded).toHaveAttribute('data-run-count', '3');
    const head = folded.querySelector('[data-role="row-head"]') as HTMLElement;
    expect(head).toHaveTextContent(/^Script call · \.bit Time Index State · \.bit Time Info/);
    // the newest time, the first row's (a feed is newest first), said as the newest
    expect(folded.querySelector('[data-role="evidence"]')).toHaveTextContent(
      `newest ${formatTimeAgo(run[0].timestamp, NOW)}`
    );
    // its party, as every row writes it; no amount and no total: nothing is summed
    expect(folded).toHaveTextContent(`ckb1…${run[0].participants[0].address!.slice(-8)}`);
    expect(folded.querySelector('[data-ckb-amount]')).toBeNull();
    expect(folded).not.toHaveTextContent('CKB');
    // the control that opens it, a real button, saying how many rows it holds
    const toggle = within(folded).getByRole('button', { name: '3 transactions ↓' });
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
    const toggle = screen.getByRole('button', { name: '3 transactions ↓' });
    await user.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(toggle).toHaveTextContent('fewer ↑');
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

  it('never folds across a row that differs: the three oracles of mainnet take turns', async () => {
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
    expect(document.querySelector('[data-role="activity-run"]')).toBeNull();
    expect(document.querySelectorAll('article[data-feed-row]')).toHaveLength(4);
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
    const toggle = await screen.findByRole('button', { name: '3 transactions ↓' });
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
    const toggle = await screen.findByRole('button', { name: '3 transactions ↓' });
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

describe('an older page that continues the last run (B4 fix round)', () => {
  /** The stream's first page (more after it), then the older page; the head poll finds nothing. */
  function pages(first: GlobalActivity[], older: GlobalActivity[]) {
    vi.mocked(api.getGlobalActivities).mockImplementation(async (params) =>
      params?.cursor === 'older'
        ? { data: older, limit: 50, hasMore: false, nextCursor: null }
        : { data: first, limit: 50, hasMore: true, nextCursor: 'older' }
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
    await user.click(screen.getByRole('button', { name: 'Older' }));
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
    const toggle = await screen.findByRole('button', { name: '2 transactions ↓' });
    await user.click(screen.getByRole('button', { name: 'Older' }));
    await waitFor(() => expect(toggle).toHaveTextContent('3 transactions ↓'));
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(feedRows()).toEqual([transfer.txHash]);
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
        loadTransactions={async () => ({
          data: [],
          total: 0,
          limit: 50,
          hasMore: false,
          nextCursor: null,
        })}
      />
    );
    const folded = await screen.findByRole('button', { name: '2 transactions ↓' });
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
        loadTransactions={async () => ({
          data: [],
          total: 0,
          limit: 50,
          hasMore: false,
          nextCursor: null,
        })}
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
