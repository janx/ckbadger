import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DaoSources } from '@/components/dao/dao-sources';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
  api: { getGlobalActivities: vi.fn(), getTransactions: vi.fn() },
}));

const tx = '0x' + 'a'.repeat(64);
const party = (n: number, ckbDelta: string, usedDelta: string) => ({
  address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfzyg3zyg3zyg3zyg3zyg3zyg3zygssparty${n}`,
  lockHash: '0x' + String(n).repeat(64),
  lockHashPrefix: null,
  lockKind: 'Default Lock',
  roles: [],
  ckbDelta,
  usedDelta,
  itemDeltas: [],
  tags: 16,
});
/** A withdrawal request as the board draws it: the request, the fee payer, and a zero-delta party. */
const activity = {
  txHash: tx,
  txIndex: 2,
  blockNumber: 123,
  // The activity API sends chain time as milliseconds; the transaction API as RFC 3339.
  timestamp: '1793268770452',
  isCellbase: false,
  participants: [
    party(1, '65296000000', '6100000000'),
    party(2, '-65296000000', '-6100000000'),
    party(3, '0', '0'),
  ],
  typeCalls: [],
  lockCalls: [],
  protocolActions: [
    {
      protocol: 'dao',
      protocolName: 'Nervos DAO',
      action: 'withdraw_request',
      metadata: { capacity: '2605000000000', depositBlock: '19695550' },
    },
  ],
};
const three = Array.from({ length: 3 }, (_, i) => ({
  ...activity,
  txHash: '0x' + String(i + 1).repeat(64),
  blockNumber: 123 - i,
}));

describe('DaoSources: Recent DAO activity (hierarchy D7, R7; v3 R15)', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: three,
      limit: 3,
      total: null,
      hasMore: true,
      nextCursor: 'next',
    } as any);
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
      'All DAO activity →',
      'DAO transactions →',
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

  it('links all DAO activity and DAO transactions on their list pages', () => {
    render(<DaoSources />);
    const section = screen.getByRole('region', { name: 'Recent DAO activity' });
    expect(within(section).getByRole('link', { name: 'All DAO activity →' })).toHaveAttribute(
      'href',
      '/mainnet/activities?filter=dao'
    );
    expect(within(section).getByRole('link', { name: 'DAO transactions →' })).toHaveAttribute(
      'href',
      '/mainnet/transactions?kind=dao'
    );
  });

  it('draws each row with the shared activity row, every participant shown (DAO-13)', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [activity],
      limit: 3,
      total: null,
      hasMore: false,
      nextCursor: null,
    } as any);
    render(<DaoSources />);
    const row = await screen.findByText(/withdraw request/i);
    const article = row.closest('article[data-feed-row]')!;
    expect(article).toHaveAttribute('data-feed-row', tx);
    // The vault ring and the action's own amounts come with the shared row.
    expect(article.querySelector('svg[data-class="dao"]')).not.toBeNull();
    expect(article).toHaveTextContent('26,050');
    // ACTIVITY_DESIGN "Show All Layers": all three parties, the zero-delta one included.
    expect(article.querySelectorAll('[data-role="participant"]')).toHaveLength(3);
    for (const n of [1, 2, 3]) expect(article).toHaveTextContent(`party${n}`);
    // The lock kind sits in its own column (one line per party), as on the activities page.
    expect(article.querySelectorAll('[data-role="lock-kind"]')).toHaveLength(3);
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
});
