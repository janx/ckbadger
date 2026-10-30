import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ForkDetailPage from '@/app/forks/[height]/[orphan]/client-page';
import { api } from '@/lib/api';
import uncleFixture from '@/__tests__/fixtures/fork-api-producer/fork-uncle-detail.json';
import reentryFixture from '@/__tests__/fixtures/fork-api-producer/fork-reentry-detail.json';
import unincludedFixture from '@/__tests__/fixtures/fork-api-producer/fork-unincluded-detail.json';
import multiFixture from '@/__tests__/fixtures/fork-api-producer/fork-multi-uncle-detail.json';
import deepFixture from '@/__tests__/fixtures/fork-api-producer/fork-deep-detail.json';

const route = { height: '10', orphan: '17404795' };
const notFound = vi.fn();
vi.mock('@/src/navigation', () => ({
  notFound: () => notFound(),
  useParams: () => route,
  usePathname: () => `/forks/${route.height}/${route.orphan}`,
}));
vi.mock('@/lib/api', () => ({
  api: { getForkAt: vi.fn() },
  isNotFoundApiError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'status' in error && error.status === 404,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const fixture = (value: unknown) => value as unknown as Awaited<ReturnType<typeof api.getForkAt>>;
const at = (height: string, orphan: string) => Object.assign(route, { height, orphan });

describe('ForkDetailPage (D7 route /forks/<height>/<orphan8>)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    at('10', '17404795');
  });

  it('asks the API for the fork at that height and orphan and titles it "Fork at #N" (FF-D3)', async () => {
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(uncleFixture));
    render(<ForkDetailPage />);
    expect(
      await screen.findByRole('heading', { level: 1, name: 'Fork at #10' })
    ).toBeInTheDocument();
    expect(api.getForkAt).toHaveBeenCalledWith(10, '17404795');
    expect(
      screen.getByText(
        '1 block deep · detected 2025-08-01 06:13:20 UTC+8 · this node switched once'
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/Fork event #1754000000000/)).toBeNull();
    expect(
      within(screen.getByRole('navigation', { name: 'Breadcrumb' })).getByRole('link', {
        name: 'Forks',
      })
    ).toHaveAttribute('href', '/mainnet/forks');
  });

  it('tells what happened in one paragraph from the API facts (FF-D4)', async () => {
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(uncleFixture));
    render(<ForkDetailPage />);
    const story = await screen.findByTestId('fork-narrative');
    expect(story).toHaveTextContent('On 2023-11-15 two blocks were found at #11, 50 ms apart.');
    expect(story).toHaveTextContent(
      'This node had followed the block that lost and switched to the other branch.'
    );
    expect(story).toHaveTextContent('#12 included the dropped block as an uncle.');
    expect(story).toHaveTextContent('1 of its 1 transaction was committed later.');
  });

  it('draws the two branches with the dropped block dashed, the uncle link and ① ② tip marks (FF-D4)', async () => {
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(uncleFixture));
    const { container } = render(<ForkDetailPage />);
    expect(await screen.findByRole('heading', { name: 'The two branches' })).toBeInTheDocument();
    const tree = container.querySelector('svg[data-fork-tree]')!;
    expect(tree.querySelectorAll('[data-tile]')).toHaveLength(4);
    expect(tree.querySelectorAll('[data-tile][data-dropped]')).toHaveLength(1);
    expect(tree.querySelectorAll('[data-uncle-link]')).toHaveLength(1);
    expect(tree.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    expect(screen.getByText(/the node switches to #12/)).toBeInTheDocument();
  });

  it('lists the blocks in one table with miner, transactions and fate, and the uncle sentence (FF-D5, FF-D6)', async () => {
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(uncleFixture));
    render(<ForkDetailPage />);
    const table = await screen.findByRole('table', { name: 'Blocks' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Height', 'Hash', 'Time (UTC+8)', 'Miner', 'Txs', 'Fate']);
    expect(within(table).getByText('#11′')).toBeInTheDocument();
    expect(within(table).getByText('uncle in #12')).toBeInTheDocument();
    expect(within(table).getAllByText('main chain')).toHaveLength(2);
    expect(within(table).getByText('06:13:20.050')).toBeInTheDocument();
    expect(
      screen.getByText(/An uncle earns its miner nothing; it counts toward the uncle rate/)
    ).toBeInTheDocument();
    expect(screen.queryByText('1 blocks')).toBeNull();
  });

  it('keeps the stored event and the uncles in tier 2 (FF-D5)', async () => {
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(uncleFixture));
    const { container } = render(<ForkDetailPage />);
    const heading = await screen.findByRole('heading', {
      name: 'This node’s fork event, as stored',
    });
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toContainElement(heading);
    expect(heading).toHaveClass('text-[13px]');
    expect(within(tier2).getByRole('heading', { name: 'Uncles in #12' })).toBeInTheDocument();
    expect(
      within(tier2).getByText(`${uncleFixture.orphanedBlocks[0].hash} · #11`)
    ).toBeInTheDocument();
  });

  it('keeps the originally adopted branch distinct from a block that came back', async () => {
    at('10', '028ea506');
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(reentryFixture));
    render(<ForkDetailPage />);
    const story = await screen.findByTestId('fork-narrative');
    expect(story).toHaveTextContent(
      'The block this node dropped at #11 is back on the main chain.'
    );
    expect(story).toHaveTextContent('A later switch on this node reversed it.');
    const table = screen.getByRole('table', { name: 'Blocks' });
    expect(within(table).getByText('adopted then, dropped since')).toBeInTheDocument();
  });

  it('names both blocks of a two-block branch taken as uncles by one block', async () => {
    at('10', '95098049');
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(multiFixture));
    render(<ForkDetailPage />);
    const story = await screen.findByTestId('fork-narrative');
    expect(story).toHaveTextContent('#13 included both dropped blocks as uncles.');
    expect(story).toHaveTextContent(
      'The dropped blocks held only their cellbases, so no transaction was undone.'
    );
  });

  it('does not link an uncommitted orphan transaction to a nonexistent canonical page', async () => {
    at('10', '36bac5b4');
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(unincludedFixture));
    render(<ForkDetailPage />);
    await waitFor(() =>
      expect(screen.getAllByText('not committed as of tip').length).toBeGreaterThan(0)
    );
    const orphanHash = unincludedFixture.orphanedTransactions[1].hash;
    expect(screen.queryByRole('link', { name: orphanHash })).not.toBeInTheDocument();
    expect(await screen.findByTestId('fork-narrative')).toHaveTextContent(
      '#11′ was not taken as an uncle.'
    );
  });

  it('refuses an answer that is not the fork the address names', async () => {
    at('10', 'deadbeef');
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(uncleFixture));
    render(<ForkDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'API answered /forks/10/deadbeef with the fork at #10 whose first orphan is 0x17404795'
    );
  });

  it('refuses a deep detection: it has no orphaned block to be addressed by', async () => {
    vi.mocked(api.getForkAt).mockResolvedValue(fixture(deepFixture));
    render(<ForkDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('with no orphaned block');
  });

  it('shows typed fork evidence only for an actual 404', async () => {
    vi.mocked(api.getForkAt).mockRejectedValue(
      Object.assign(new Error('absent fork'), { status: 404 })
    );
    render(<ForkDetailPage />);
    expect((await screen.findByText('checked')).nextElementSibling).toHaveTextContent(
      '/forks/10/17404795'
    );
  });

  it('sends a malformed address (the old millisecond id) to not-found without a request', () => {
    at('1754000000000', '');
    render(<ForkDetailPage />);
    expect(notFound).toHaveBeenCalled();
    expect(api.getForkAt).not.toHaveBeenCalled();
  });
});
