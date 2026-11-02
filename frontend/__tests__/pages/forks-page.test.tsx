import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
import ForksPage from '@/app/forks/page';
import { api } from '@/lib/api';
import listFixture from '@/__tests__/fixtures/fork-api-producer/fork-list.json';
import overviewFixture from '@/__tests__/fixtures/fork-api-producer/fork-overview.json';
import deepFixture from '@/__tests__/fixtures/fork-api-producer/fork-deep-detail.json';

vi.mock('@/lib/api', () => ({
  api: { getForks: vi.fn(), getForkOverview: vi.fn() },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

type List = Awaited<ReturnType<typeof api.getForks>>;
const list = () => listFixture as unknown as List;
const overview = () =>
  overviewFixture as unknown as Awaited<ReturnType<typeof api.getForkOverview>>;
const table = () => screen.getByRole('table', { name: 'Branch switches on this node' });

describe('ForksPage', () => {
  beforeEach(() => vi.clearAllMocks());

  it('leads with the uncle rate as the one display figure, the days under it, the node’s record in one quiet row (FF-L1, R11, R13)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    const { container } = render(<ForksPage />);
    const hero = screen.getByRole('region', { name: 'Forks' });
    expect(within(hero).getByRole('heading', { level: 1, name: 'Forks' })).toHaveClass(
      'text-index'
    );
    // R13: one short lede, dim, ≤ 90 characters, with the way down to tier 2
    const lede = within(hero).getByText(/^The chain keeps one of two rival blocks/);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    expect(lede.textContent!.length).toBeLessThanOrEqual(90);
    expect(within(lede).getByRole('link', { name: 'How it works ↓' })).toHaveAttribute(
      'href',
      '#evidence'
    );
    // the figure: the window's uncle rate, bright (Forks has no class colour, R11), against the target
    const rate = await within(hero).findByText('250.00');
    expect(rate).toHaveClass('sm:text-display', 'text-text-bright');
    expect(within(rate).getByText('%')).toHaveClass('text-hero');
    expect(within(hero).getByText('uncle rate · 3 days through 2026-10-22')).toBeInTheDocument();
    expect(within(hero).getByText('5 uncles in 2 blocks · target 2.50%')).toHaveClass('text-body');
    // the days are the picture, inside the hero section
    expect(hero.querySelectorAll('[data-uncles]')).toHaveLength(3);
    // the node's own record (R2b-b): one quiet row under the reading, outside the hero
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(3));
    expect(screen.getAllByRole('term').map((t) => t.textContent)).toEqual([
      'This node switched branch',
      'Its orphans',
      'Orphaned transactions',
    ]);
    const row = screen.getAllByRole('term')[0].closest('dl')!;
    expect(hero.contains(row)).toBe(false);
    expect(
      within(row)
        .getAllByRole('definition')
        .map((dd) => dd.textContent)
    ).toEqual([
      '1 time · since 2025-08-01',
      '1 uncle · 1 one block later',
      '1 of 1 · committed in a later block',
    ]);
    // R11: no coloured text in tier 1 (the glyphs and bars keep their colours)
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    await waitFor(() => expect(table()).toBeInTheDocument());
    const coloured = Array.from(tier1.querySelectorAll('[class]')).filter((el) =>
      el
        .getAttribute('class')!
        .split(/\s+/)
        .some((c) => /^text-(jade|gold|rouge)(\/\d+)?$/.test(c))
    );
    expect(coloured).toEqual([]);
    expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
    // the full concept sentence lives in tier 2
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveTextContent(/aiming at one in 40 blocks\./);
    expect(tier1).not.toHaveTextContent(/aiming at one in 40 blocks/);
    expect(api.getForkOverview).toHaveBeenCalledWith(30);
  });

  it('claims nothing about orphans before the node has switched at all (visual pass, live empty store)', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [],
      total: 0,
      hasMore: false,
      nextCursor: null,
    });
    const record = overview().nodeRecord;
    const fates = record.orphanFates!;
    vi.mocked(api.getForkOverview).mockResolvedValue({
      ...overview(),
      nodeSwitchesAllTime: 0,
      nodeSwitchesInWindow: 0,
      days: overview().days.map((day) => ({ ...day, nodeSwitches: 0 })),
      nodeRecord: {
        ...record,
        orphanedBlocks: 0,
        orphanedTransactions: 0,
        orphanFates: {
          ...fates,
          eventsWithEvidence: 0,
          blocks: { ...fates.blocks, total: 0, includedAsUncle: 0, uncleDistances: [] },
          userTransactions: { ...fates.userTransactions, total: 0, committedLater: 0 },
          competing: { compared: 0, sameMiner: 0, minerUnknown: 0, gapMedianMs: null },
        },
      },
    });
    render(<ForksPage />);
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(1));
    expect(screen.queryByText('Its orphans')).toBeNull();
    expect(screen.queryByText(/held only cellbases/)).toBeNull();
    expect(screen.queryByText(/0 of 0 were one miner/)).toBeNull();
  });

  it('leaves out the orphan figures when the node reader is not open, and says so', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue({
      ...overview(),
      nodeRecord: {
        ...overview().nodeRecord,
        evidenceStatus: 'node_reader_unavailable',
        orphanFates: null,
        switchesDeeperThanOneBlock: 2,
      },
    });
    render(<ForksPage />);
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(1));
    expect(screen.getByRole('definition')).toHaveTextContent(
      /^1 time · since 2025-08-01, 2 of them more than one block deep$/
    );
    expect(
      screen.getByText(/what became of the dropped blocks needs the node reader/)
    ).toBeInTheDocument();
  });

  it('draws the days as grey uncle bars with jade switches; one reading and one caption under them, the rest in tier 2 (FF-L2, F3)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    const { container } = render(<ForksPage />);
    await waitFor(() => expect(container.querySelectorAll('[data-uncles]')).toHaveLength(3));
    expect(container.querySelectorAll('[data-switches]')).toHaveLength(1);
    // hierarchy v3: the picture has no separate heading
    expect(
      screen.queryByRole('heading', { name: 'The chain’s record and this node’s view' })
    ).toBeNull();
    // under the days: the reading row, then one caption (≤ 2 lines of text, F3)
    const reading = screen.getByRole('status', { name: 'Day readout' });
    const under = reading.closest('[data-role="reading"]') as HTMLElement;
    expect(Array.from(under.children).map((el) => el.tagName)).toEqual(['DIV', 'P']);
    const caption = under.querySelector('p')!;
    expect(caption).toHaveClass('text-body', 'max-w-[640px]');
    expect(caption.textContent!.length * 8).toBeLessThanOrEqual(640);
    // the paragraph and the scope note moved to tier 2, every sentence kept
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(
      within(tier2).getByText(
        /The chain recorded 5 uncles in these 3 days; this node switched branches 1 time in the same days\. It only notices a fork when the losing block reached it first and it had to switch\. The uncles are the complete record, and they are on chain for anyone to recount\./
      )
    ).toBeInTheDocument();
    expect(
      within(tier2).getByText(
        /indexed chain time, node switches use this indexer's detection wall time/
      )
    ).toBeInTheDocument();
    expect(within(tier2).getByText(/focus the chart and use ← →/)).toBeInTheDocument();
  });

  it('gives every switch a fork glyph, the miners, the gap and its fate, linked by D7 (FF-L3, FF-L4)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(
      within(table())
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual([
      'Seen (UTC+8)',
      'Fork point',
      'Shape',
      'Dropped block’s miner vs winner’s',
      'Found apart',
      'What became of it',
    ]);
    const seen = within(table()).getByRole('link', { name: '2025-08-01 06:13:20' });
    expect(seen).toHaveAttribute('href', '/mainnet/forks/10/17404795');
    // R14: the list's record note left the h2 row for tier 2; its key stays, in neutral words
    expect(
      within(document.querySelector('[data-tier="2"]') as HTMLElement).getByText(
        '0 of 1 were one miner against itself · median gap 50 ms'
      )
    ).toBeInTheDocument();
    const key = screen.getByRole('list', { name: 'Shape key' });
    expect(key).toHaveTextContent(
      'dashed: the block it dropped→ the block that took it as an uncle'
    );
    expect(key.querySelector('.text-jade')).toBeNull();
    expect(within(table()).getByText('depth 1')).toBeInTheDocument();
    expect(table().querySelector('[data-uncle-link]')).not.toBeNull();
    expect(within(table()).getByText('50 ms')).toBeInTheDocument();
    // R11: the fate reads in bright ink; the glyph's arrow keeps the jade
    const fate = within(table()).getByText('uncle in #12 (+1)');
    expect(fate).toHaveClass('text-text-bright');
    expect(fate).not.toHaveClass('text-jade');
    expect(within(table()).getByText('1 of 1 tx committed later')).toBeInTheDocument();
    expect(within(table()).getAllByText('witness unavailable')).toHaveLength(2);
    expect(api.getForks).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
  });

  it('names both miners by address tail and marks a fork one miner had against itself (FF-L3)', async () => {
    const miner = `0x${'ab'.repeat(32)}`;
    const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';
    const event = list().data[0];
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [
        {
          ...event,
          preview: {
            ...event.preview!,
            oldMinerLockHash: miner,
            oldMinerAddress: address,
            competingMinerLockHash: miner,
            competingMinerAddress: address,
          },
        },
      ],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByText('same miner')).toBeInTheDocument();
    const miners = within(table()).getAllByRole('link', { name: '…93hhsj' });
    expect(miners).toHaveLength(2);
    expect(miners[0]).toHaveAttribute('href', `/mainnet/address/${address}`);
  });

  it('names a miner by its pool when pools.toml does, else by address tail (R-12)', async () => {
    const event = list().data[0];
    const address = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw93hhsj';
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [
        {
          ...event,
          preview: {
            ...event.preview!,
            oldMinerLockHash: `0x${'ab'.repeat(32)}`,
            oldMinerAddress: address,
            oldMinerPool: 'Example Pool',
            competingMinerLockHash: `0x${'cd'.repeat(32)}`,
            competingMinerAddress: address.replace('w93hhsj', 'q2cdyx8'),
            competingMinerPool: null,
          },
        },
      ],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByRole('link', { name: 'Example Pool' })).toHaveAttribute(
      'href',
      `/mainnet/address/${address}`
    );
    expect(within(table()).getByRole('link', { name: '…2cdyx8' })).toBeInTheDocument();
  });

  it('links a row from firstOrphanHash even without the node preview', async () => {
    const event = list().data[0];
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [{ ...event, preview: null, previewStatus: 'node_reader_unavailable' }],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByRole('link', { name: '2025-08-01 06:13:20' })).toHaveAttribute(
      'href',
      '/mainnet/forks/10/17404795'
    );
  });

  it('distinguishes no node switches from an overview load failure', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [],
      total: 0,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getForkOverview).mockRejectedValue(new Error('missing daily block stats'));
    render(<ForksPage />);
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('missing daily block stats')
    );
    expect(screen.getByText(/No branch switches observed by this node/)).toBeInTheDocument();
  });

  it('labels a deep detection as no rollback, without a route it cannot have', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [{ ...deepFixture.event, previewStatus: 'no_old_branch', preview: null }],
    } as unknown as List);
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() =>
      expect(within(table()).getByText('No rollback at detection')).toBeInTheDocument()
    );
    expect(within(table()).getByText('indexed #12 → node #13')).toBeInTheDocument();
    expect(within(table()).queryByRole('link', { name: /^20\d\d-/ })).toBeNull();
    expect(
      within(table()).getByText('nothing was rolled back, so no detail page')
    ).toBeInTheDocument();
  });
});
