import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
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

const list = () => listFixture as unknown as Awaited<ReturnType<typeof api.getForks>>;
const overview = () =>
  overviewFixture as unknown as Awaited<ReturnType<typeof api.getForkOverview>>;

describe('ForksPage', () => {
  beforeEach(() => vi.clearAllMocks());
  it('shows exact complete-day counts and current old-branch fate from producer JSON', async () => {
    vi.mocked(api.getForks).mockResolvedValue(list());
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    const title = await screen.findByRole('heading', { level: 1, name: 'Forks' });
    expect(title).toHaveClass('text-[26px]');
    await waitFor(() => expect(screen.getAllByText('included as uncle').length).toBeGreaterThan(0));
    expect(screen.getAllByText(/Old: witness unavailable/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/\+50 ms/).length).toBeGreaterThan(0);
    expect(screen.getByText('Uncles · 30 completed days')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
    expect(
      screen.getByText(/indexed chain time, node switches use this indexer's detection wall time/)
    ).toBeInTheDocument();
    expect(api.getForks).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    expect(api.getForkOverview).toHaveBeenCalledWith(30);
    expect(screen.getAllByText(/UTC\+8/).length).toBeGreaterThan(0);
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
  it('labels a deep detection as no rollback instead of an adopted branch', async () => {
    vi.mocked(api.getForks).mockResolvedValue({
      ...list(),
      data: [{ ...deepFixture.event, previewStatus: 'no_old_branch', preview: null }],
    } as Awaited<ReturnType<typeof api.getForks>>);
    vi.mocked(api.getForkOverview).mockResolvedValue(overview());
    render(<ForksPage />);
    await waitFor(() =>
      expect(screen.getAllByText('No rollback at detection').length).toBeGreaterThan(0)
    );
    expect(screen.getByText(/indexed #12 → node #13/)).toBeInTheDocument();
  });
});
