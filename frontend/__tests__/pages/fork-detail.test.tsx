import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ForkDetailPage from '@/app/forks/[id]/client-page';
import { api } from '@/lib/api';
import uncleFixture from '@/__tests__/fixtures/fork-api-producer/fork-uncle-detail.json';
import reentryFixture from '@/__tests__/fixtures/fork-api-producer/fork-reentry-detail.json';
import unincludedFixture from '@/__tests__/fixtures/fork-api-producer/fork-unincluded-detail.json';
import deepFixture from '@/__tests__/fixtures/fork-api-producer/fork-deep-detail.json';

vi.mock('@/src/navigation', () => ({
  notFound: vi.fn(),
  useParams: () => ({ id: '1754000000000' }),
  usePathname: () => '/forks/1754000000000',
}));
vi.mock('@/lib/api', () => ({
  api: { getForkDetail: vi.fn() },
  isNotFoundApiError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'status' in error && error.status === 404,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const fixture = (value: unknown) =>
  value as unknown as Awaited<ReturnType<typeof api.getForkDetail>>;

describe('ForkDetailPage', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders both historical branches and exact uncle, competing-time, and later-commit evidence', async () => {
    vi.mocked(api.getForkDetail).mockResolvedValue(fixture(uncleFixture));
    render(<ForkDetailPage />);
    await waitFor(() => expect(screen.getByText('Fork event #1754000000000')).toBeInTheDocument());
    expect(screen.getByText('Two branches at this switch')).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Historical old branch' })).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Historically adopted branch' })).toBeInTheDocument();
    expect(screen.getAllByText('included as uncle').length).toBeGreaterThan(0);
    expect(screen.getAllByText('+50 ms').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Miner witness unavailable').length).toBeGreaterThan(0);
    expect(screen.getAllByText('committed later').length).toBeGreaterThan(0);
    expect(screen.getAllByText('0.00001234 CKB').length).toBeGreaterThan(0);
  });

  it('keeps the originally adopted branch distinct from a later reentered canonical block', async () => {
    vi.mocked(api.getForkDetail).mockResolvedValue(fixture(reentryFixture));
    render(<ForkDetailPage />);
    await waitFor(() => expect(screen.getByText('Fork event #1754000000000')).toBeInTheDocument());
    expect(screen.getAllByText('canonical now').length).toBeGreaterThan(0);
    expect(screen.getAllByText('This old block re-entered').length).toBeGreaterThan(0);
    expect(screen.getByText('Current canonical view at the affected heights')).toBeInTheDocument();
    expect(reentryFixture.adoptedBranch[0].hash).not.toBe(reentryFixture.canonicalBranch[0].hash);
  });

  it('explains when the observed adopted tip was superseded before replay', async () => {
    const supersedingHash = `0x${'dd'.repeat(32)}`;
    vi.mocked(api.getForkDetail).mockResolvedValue(
      fixture({
        ...reentryFixture,
        event: { ...reentryFixture.event, supersededByTipHash: supersedingHash },
      })
    );
    render(<ForkDetailPage />);
    expect(await screen.findByText(/superseded before replay/)).toHaveTextContent('#11');
    expect(screen.getByText(supersedingHash)).toBeInTheDocument();
  });

  it('shows interruption separately from an already superseded observation', async () => {
    vi.mocked(api.getForkDetail).mockResolvedValue(
      fixture({
        ...reentryFixture,
        event: {
          ...reentryFixture.event,
          reconcileInterrupted: true,
          supersededByTipHash: `0x${'dd'.repeat(32)}`,
        },
      })
    );
    render(<ForkDetailPage />);
    expect(
      await screen.findByText('Reconciliation interrupted by a later rollback')
    ).toBeInTheDocument();
    expect(screen.getByText(/superseded before replay/)).toBeInTheDocument();
  });

  it('surfaces an evidence read error', async () => {
    vi.mocked(api.getForkDetail).mockRejectedValue(new Error('missing adopted block 11'));
    render(<ForkDetailPage />);
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('missing adopted block 11')
    );
  });

  it('shows typed fork evidence only for an actual 404', async () => {
    vi.mocked(api.getForkDetail).mockRejectedValue(
      Object.assign(new Error('absent fork'), { status: 404 })
    );
    render(<ForkDetailPage />);
    expect((await screen.findByText('checked')).nextElementSibling).toHaveTextContent(
      '/forks/1754000000000'
    );
    expect(
      screen.getByText('This explorer could not open the fork at this address.')
    ).toBeInTheDocument();
  });

  it('does not link an uncommitted orphan transaction to a nonexistent canonical transaction page', async () => {
    vi.mocked(api.getForkDetail).mockResolvedValue(fixture(unincludedFixture));
    render(<ForkDetailPage />);
    await waitFor(() =>
      expect(screen.getAllByText('not committed as of tip').length).toBeGreaterThan(0)
    );
    const orphanHash = unincludedFixture.orphanedTransactions[1].hash;
    expect(screen.queryByRole('link', { name: orphanHash })).not.toBeInTheDocument();
  });

  it('describes a deep detection as a pause without claiming either branch was adopted or dropped', async () => {
    vi.mocked(api.getForkDetail).mockResolvedValue(fixture(deepFixture));
    render(<ForkDetailPage />);
    expect(await screen.findByText('Two tips at detection')).toBeInTheDocument();
    expect(screen.getByText(/paused indexing before rollback/)).toBeInTheDocument();
    expect(screen.getByText('Indexed tip · no rollback')).toBeInTheDocument();
    expect(screen.getByText('Node tip · not adopted by indexer')).toBeInTheDocument();
    expect(screen.queryByText('Old branch · dropped at detection')).not.toBeInTheDocument();
  });
});
