import { seededAddress } from '@/__tests__/utils/ckb-address';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DaoSources } from '@/components/dao/dao-sources';
import { api } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';
import { activityRead } from '@/__tests__/utils/activity-read';

vi.mock('@/lib/api', () => ({
  api: { getGlobalActivities: vi.fn(), getTransactions: vi.fn() },
}));

const tx = '0x' + 'a'.repeat(64);
/** A party; `sent` says whether its lock is an input's (the API's `sent`). */
const party = (n: number, ckbDelta: string, usedDelta: string, sent: boolean) => ({
  address: seededAddress(`party${n}`),
  lockHash: '0x' + String(n).repeat(64),
  lockHashPrefix: null,
  lockKind: 'Default Lock',
  roles: [],
  ckbDelta,
  usedDelta,
  itemDeltas: [],
  tags: 16,
  sent,
});
/**
 * A withdrawal request as the board draws it: the request (the depositor spends its deposit cell
 * into the request cell, so it sent), the fee payer (sent), and a zero-delta party (did not).
 */
const activity = activityRead({
  txHash: tx,
  txIndex: 2,
  blockNumber: 123,
  // The activity API sends chain time as milliseconds; the transaction API as RFC 3339.
  timestamp: '1793268770452',
  isCellbase: false,
  participants: [
    party(1, '65296000000', '6100000000', true),
    party(2, '-65296000000', '-6100000000', true),
    party(3, '0', '0', false),
  ],
  typeCalls: [],
  lockCalls: [],
  // the Nervos DAO's type script, the recognised code that ran in a DAO step (C7)
  recognizedTypes: [
    {
      typeCodeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
      typeHashType: 'type',
      scriptName: 'Nervos DAO',
    },
  ],
  protocolActions: [
    {
      protocol: 'dao',
      protocolName: 'Nervos DAO',
      action: 'withdraw_request',
      metadata: { capacity: '2605000000000', depositBlock: '19695550' },
    },
  ],
});
// B4 (re-pinned): three requests by three depositors. Re-pinned on purpose (Gate B's rule, lists
// lane P3): rows that did the same thing and called the same code fold whoever the parties are, so
// three requests running are one run; the newest three here are a request, a deposit and a
// request, which do not (the run is pinned below)
const request = (i: number) =>
  activityRead({
    ...activity,
    txHash: '0x' + String(i + 1).repeat(64),
    blockNumber: 123 - i,
    participants: [
      party(4 + i, '65296000000', '6100000000', true),
      ...activity.participants.slice(1),
    ],
  });
const deposit = (i: number) =>
  activityRead({
    ...request(i),
    protocolActions: [
      {
        protocol: 'dao',
        protocolName: 'Nervos DAO',
        action: 'deposit',
        metadata: { capacity: '2605000000000' },
      },
    ],
  });
const three = [request(0), deposit(1), request(2)];

describe('DaoSources: Recent DAO activity (hierarchy D7, R7; v3 R15)', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.getGlobalActivities).mockResolvedValue(
      pagedRead({
        data: three,
        limit: 3,
        total: null,
        hasMore: true,
        nextCursor: 'next',
      }) as any
    );
  });

  it('loads the three newest DAO activities under one heading, with no tabs, no pager and no note (R15)', async () => {
    render(<DaoSources />);
    await waitFor(() =>
      expect(api.getGlobalActivities).toHaveBeenCalledWith({ filter: 'dao', limit: 3 })
    );
    const section = screen.getByRole('region', { name: 'Recent DAO activity' });
    // Phase 5 (B1): the feed as a command run on its row count, the --last flag its parameter.
    const heading = within(section).getByRole('heading', {
      level: 2,
      name: 'dao activity --last 3',
    });
    expect(heading).toHaveClass('prompt', 'text-section', 'text-text-bright');
    expect(heading.lastElementChild).toHaveTextContent(/^--last 3$/);
    expect(heading.lastElementChild).toHaveClass('text-text-dim');
    // The note under the heading moved to tier 2 (dao-board); only the two links stay beside it,
    // as the right-prompt note, outside the heading's name.
    expect(section).not.toHaveTextContent(/newest first/);
    const note = heading.nextElementSibling as HTMLElement;
    expect(note).toHaveAttribute('data-prompt-note');
    expect(heading).not.toContainElement(note);
    expect(Array.from(note.firstElementChild!.children).map((c) => c.textContent)).toEqual([
      // re-pinned (lane P4): a way onward's arrow is the onward look's, aria-hidden and held to
      // its last word by a no-break space (it was typed into the words)
      'All DAO activity\u00a0→',
      'DAO transactions\u00a0→',
    ]);
    await waitFor(() => expect(section.querySelectorAll('article[data-feed-row]')).toHaveLength(3));
    // Tight rows: 10 px above and below each (the shared row keeps its 2 px between party lines).
    for (const row of Array.from(section.querySelectorAll('article[data-feed-row]')))
      expect(row).toHaveClass('py-2.5');
    // R7: no Activities / Transactions tabs and no "Older …" pager; the list pages page instead.
    expect(screen.queryByRole('tablist')).toBeNull();
    expect(screen.queryByRole('tab')).toBeNull();
    expect(screen.queryByRole('button', { name: /older/i })).toBeNull();
    expect(screen.queryByRole('table')).toBeNull();
    expect(api.getTransactions).not.toHaveBeenCalled();
  });

  it('folds three requests running into one run, whoever made them (Gate B)', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(
      pagedRead({
        data: [request(0), request(1), request(2)],
        limit: 3,
        total: null,
        hasMore: true,
        nextCursor: 'next',
      }) as any
    );
    render(<DaoSources />);
    const section = screen.getByRole('region', { name: 'Recent DAO activity' });
    await waitFor(() =>
      expect(section.querySelector('[data-role="activity-run"]')).toHaveAttribute(
        'data-run-count',
        '3'
      )
    );
    // three depositors, and the fee payer every request shares; the zero-delta party did not send
    // (re-pinned on purpose, resync/int: the run counts its senders, it counted 5 participants);
    // re-pinned on purpose (the lead's finding, the exactness lane's B9): the three newest, older
    // DAO activity unread (the route has more), so the run's counts are so far
    expect(section.querySelector('[data-role="run-count"]')).toHaveTextContent(
      /^3 transactions so far ↓ · 4 senders so far$/
    );
    expect(section.querySelectorAll('article[data-feed-row]')).toHaveLength(0);
  });

  it('links all DAO activity and DAO transactions on their list pages', () => {
    render(<DaoSources />);
    const section = screen.getByRole('region', { name: 'Recent DAO activity' });
    // re-pinned (lane P4): a way onward's arrow is the onward look's, aria-hidden and held to
    // its last word by a no-break space (it was typed into the words)
    expect(within(section).getByRole('link', { name: 'All DAO activity' })).toHaveAttribute(
      'href',
      '/mainnet/activities?filter=dao'
    );
    expect(within(section).getByRole('link', { name: 'DAO transactions' })).toHaveAttribute(
      'href',
      '/mainnet/transactions?kind=dao'
    );
  });

  it('draws each row with the shared activity row, every participant shown (DAO-13)', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(
      pagedRead({
        data: [activity],
        limit: 3,
        total: null,
        hasMore: false,
        nextCursor: null,
      }) as any
    );
    render(<DaoSources />);
    const row = await screen.findByText(/withdraw request/i);
    const article = row.closest('article[data-feed-row]')!;
    expect(article).toHaveAttribute('data-feed-row', tx);
    // The vault ring and the action's own amounts come with the shared row.
    expect(article.querySelector('svg[data-class="dao"]')).not.toBeNull();
    expect(article).toHaveTextContent('26,050');
    // ACTIVITY_DESIGN "Show All Layers": all three parties, the zero-delta one included.
    expect(article.querySelectorAll('[data-role="participant"]')).toHaveLength(3);
    // B4 (re-pinned): real addresses, each written `ckb1…` and its last 8 characters (R9)
    for (const p of activity.participants)
      expect(article).toHaveTextContent(`ckb1…${p.address.slice(-8)}`);
    // The lock kind sits in its own place on each party's line, as on the activities page; on the
    // default lock it is not written at all (B4, re-pinned: it was "Default Lock" three times).
    expect(article.querySelectorAll('[data-role="lock-kind"]')).toHaveLength(0);
    expect(article).not.toHaveTextContent('Default Lock');
    // R11: no gold text in a DAO row; the vault ring is the graphic that carries the colour.
    const gold = Array.from(article.querySelectorAll('[class]')).filter((el) =>
      /(^|\s)text-gold/.test(el.getAttribute('class')!)
    );
    expect(gold).toHaveLength(0);
    // Raw milliseconds never reach the page (DAO-1).
    expect(screen.queryByText(/1793268770452/)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: '#123' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/123'
    );
  });

  it('reports source errors without inventing rows', async () => {
    vi.mocked(api.getGlobalActivities).mockRejectedValue(new Error('canonical index missing'));
    render(<DaoSources />);
    expect(await screen.findByRole('alert')).toHaveTextContent('canonical index missing');
    expect(screen.queryByRole('link', { name: '#123' })).not.toBeInTheDocument();
  });

  // A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
  // awaited: the feed says it is loading, never "No DAO activities", and lists once back online.
  it('says it is loading while its read is paused, then lists the activities once back online', async () => {
    const { resume } = renderPaused(<DaoSources />);
    expect(await screen.findByText('Loading DAO activities…')).toBeInTheDocument();
    expect(screen.queryByText(/No DAO activities/)).toBeNull();
    resume();
    await waitFor(() =>
      expect(document.querySelectorAll('article[data-feed-row]')).toHaveLength(3)
    );
    expect(screen.queryByText('Loading DAO activities…')).toBeNull();
  });
});
