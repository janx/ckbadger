import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
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

  it('leads with the chain’s own record: uncle rate against the 2.5% target (FF-L1)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    expect(await screen.findByRole('heading', { level: 1, name: 'Forks' })).toHaveClass(
      'text-[26px]'
    );
    expect(screen.getByText(/aiming at one in 40 blocks/)).toBeInTheDocument();
    await waitFor(() => expect(screen.getAllByRole('term')).toHaveLength(2));
    expect(screen.getAllByRole('term').map((t) => t.textContent)).toEqual([
      'Uncle rate · 3 days through 2026-10-22',
      'This node switched branch',
    ]);
    expect(screen.getByText('250.00%')).toBeInTheDocument();
    expect(screen.getByText('5 uncles in 2 blocks · target 2.50%')).toBeInTheDocument();
    expect(screen.getByText('1 time')).toBeInTheDocument();
    expect(screen.getByText('1 in these 3 days')).toBeInTheDocument();
    expect(api.getForkOverview).toHaveBeenCalledWith(30);
  });

  it('draws the days as grey uncle bars with jade switches and says what they mean (FF-L2)', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    const { container } = render(<ForksPage />);
    expect(
      await screen.findByRole('heading', { name: 'The chain’s record and this node’s view' })
    ).toBeInTheDocument();
    await waitFor(() => expect(container.querySelectorAll('[data-uncles]')).toHaveLength(3));
    expect(container.querySelectorAll('[data-switches]')).toHaveLength(1);
    expect(
      screen.getByText(
        /The chain recorded 5 uncles in these 3 days; this node switched branches 1 time in the same days\./
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText(/indexed chain time, node switches use this indexer's detection wall time/)
    ).toBeInTheDocument();
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
    expect(within(table()).getByText('depth 1')).toBeInTheDocument();
    expect(table().querySelector('[data-uncle-link]')).not.toBeNull();
    expect(within(table()).getByText('50 ms')).toBeInTheDocument();
    expect(within(table()).getByText('uncle in #12 (+1)')).toBeInTheDocument();
    expect(within(table()).getByText('1 of 1 tx committed later')).toBeInTheDocument();
    expect(within(table()).getAllByText('witness unavailable')).toHaveLength(2);
    expect(api.getForks).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
  });

  it('marks a fork one miner had against itself (FF-L3)', async () => {
    const miner = `0x${'ab'.repeat(32)}`;
    const event = list().data[0];
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [
        {
          ...event,
          preview: { ...event.preview!, oldMinerLockHash: miner, competingMinerLockHash: miner },
        },
      ],
    });
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() => expect(table()).toBeInTheDocument());
    expect(within(table()).getByText('same miner')).toBeInTheDocument();
    expect(within(table()).getAllByText('0xababab…abab')).toHaveLength(2);
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
    } as List);
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() =>
      expect(within(table()).getByText('No rollback at detection')).toBeInTheDocument()
    );
    expect(within(table()).getByText('indexed #12 → node #13')).toBeInTheDocument();
    expect(within(table()).queryByRole('link', { name: /^20\d\d-/ })).toBeNull();
  });
});
