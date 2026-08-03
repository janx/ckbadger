import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ClusterDetailPage from '@/app/clusters/[clusterId]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeCluster: vi.fn(),
    getSporesByCluster: vi.fn(),
    getSporeClusterCapacityChart: vi.fn(),
    getSporeClusterHolders: vi.fn(),
    getSporeClusterActivities: vi.fn(),
    getAddress: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockClusterId = '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890';
let mockSearchParamsString = '';
const mockReplace = vi.fn();

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ clusterId: mockClusterId }),
  useRouter: () => ({ push: vi.fn(), replace: mockReplace }),
  usePathname: () => `/clusters/${mockClusterId}`,
  useSearchParams: () => new URLSearchParams(mockSearchParamsString),
}));

const mockCluster = {
  clusterId: mockClusterId,
  name: 'Test Collection',
  description: 'A test collection of spores',
  composition: {
    tier: 'btc_ckb' as const,
    fullyOnchainCount: 5,
    pureCkbCount: 0,
    decentralizedMixtureCount: 0,
    centralizedMixtureCount: 0,
    unknownCount: 0,
    fullyOnchainRatio: '1.0',
  },
  ownerLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
  sporesCount: 5,
  holdersCount: 3,
  activitiesCount: 10,
  createdAtBlock: 1000000,
  ownedCapacity: '100000000000',
  ownedKnowledge: '61000000000',
};

const mockSpores = {
  data: [
    {
      sporeId: '0x2222222222222222222222222222222222222222222222222222222222222222',
      txHash: '0x3333333333333333333333333333333333333333333333333333333333333333',
      outputIndex: 0,
      clusterId: mockClusterId,
      contentType: 'image/png',
      contentSize: 1024,
      ownerLockHash: '0x4444444444444444444444444444444444444444444444444444444444444444',
      ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
      isLive: true,
      createdAtBlock: 1000001,
    },
    {
      sporeId: '0x5555555555555555555555555555555555555555555555555555555555555555',
      txHash: '0x6666666666666666666666666666666666666666666666666666666666666666',
      outputIndex: 0,
      clusterId: mockClusterId,
      contentType: 'text/plain',
      contentSize: 256,
      ownerLockHash: '0x7777777777777777777777777777777777777777777777777777777777777777',
      ownerAddress: undefined,
      isLive: true,
      createdAtBlock: 1000002,
    },
  ],
  total: 2,
  limit: 18,
  hasMore: false,
  nextCursor: null,
};

const emptySpores = {
  data: [],
  total: 0,
  limit: 18,
  hasMore: false,
  nextCursor: null,
};

describe('ClusterDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSearchParamsString = '';
    vi.mocked(api.getSporeClusterCapacityChart).mockResolvedValue({
      title: 'Test Collection Capacity History',
      data: [],
      series: [],
    });
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: mockCluster.ownerLockHash,
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getSporeClusterHolders).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getSporeClusterActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders overview sections, back link, object gallery, and Activities tab by default', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByTestId('header')).toBeInTheDocument();
      expect(screen.getByText('Test Collection')).toBeInTheDocument();
      expect(screen.getByText('A test collection of spores')).toBeInTheDocument();
      expect(screen.getByText('Spore Cluster')).toBeInTheDocument();
      expect(screen.getByText('Capacity Statistics')).toBeInTheDocument();
      expect(screen.getByText('Collection Overview')).toBeInTheDocument();
      expect(screen.getByText('Composition')).toBeInTheDocument();
      expect(screen.getByText('Supply')).toBeInTheDocument();
      expect(screen.getByText('creator')).toBeInTheDocument();
      // Objects gallery is always visible (standalone panel)
      expect(screen.getByText(/^Objects \(/)).toBeInTheDocument();
      // Activities and Holders are tabs
      expect(screen.getByRole('button', { name: /^Activities \(/ })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /^Holders \(/ })).toBeInTheDocument();
      expect(screen.getByText('No activities in this collection')).toBeInTheDocument();
      // Search/filter controls are always visible in the gallery panel header
      expect(screen.getByLabelText('Search spores')).toBeInTheDocument();
      const backLink = screen.getByText('\u2190 Back to Objects');
      expect(backLink.closest('a')).toHaveAttribute('href', '/mainnet/inventory/objects');
    });
  });

  it('hydrates the requested tab and falls back to Activities for invalid tab params', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    for (const testCase of [
      {
        searchParams: 'tab=holders',
        assert: async () => {
          await waitFor(() => {
            expect(screen.getByText('No holders in this collection')).toBeInTheDocument();
          });
        },
      },
      {
        searchParams: 'tab=invalid',
        assert: async () => {
          await waitFor(() => {
            expect(screen.getByText('No activities in this collection')).toBeInTheDocument();
          });
        },
      },
    ]) {
      mockSearchParamsString = testCase.searchParams;
      const view = render(<ClusterDetailPage clusterId={mockClusterId} />);
      await testCase.assert();
      view.unmount();
    }
  });

  it('renders spore objects with content type and size in gallery cards', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      // Gallery panel renders spore cards with content type and size
      expect(screen.getAllByText('1,024 B').length).toBeGreaterThan(0);
      expect(screen.getAllByText('256 B').length).toBeGreaterThan(0);
      expect(screen.getAllByText('image/png').length).toBeGreaterThan(0);
      expect(screen.getAllByText('text/plain').length).toBeGreaterThan(0);
    });
  });

  it('handles malformed spore payload without crashing', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue({
      data: [
        {
          ...mockSpores.data[0],
          sporeId: '0x0000000000000000000000000000000000000000000000000000000000000000',
          contentType: undefined,
          ownerLockHash: undefined,
          ownerAddress: undefined,
          contentSize: undefined,
          createdAtBlock: undefined,
        } as any,
      ],
      total: 1,
      limit: 18,
      hasMore: false,
      nextCursor: null,
    });

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      // The gallery panel should render without crashing
      expect(screen.getByText(/^Objects \(/)).toBeInTheDocument();
      // Content type falls back to 'unknown'
      expect(screen.getAllByText('unknown').length).toBeGreaterThan(0);
    });
  });

  it('shows error state when cluster not found', async () => {
    vi.mocked(api.getSporeCluster).mockRejectedValue(new Error('Not found'));
    vi.mocked(api.getSporesByCluster).mockResolvedValue(emptySpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Spore Cluster not found')).toBeInTheDocument();
    });
  });

  it('shows empty state when cluster has no spores', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(emptySpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      // Gallery panel shows empty state
      expect(screen.getByText('No objects in this collection')).toBeInTheDocument();
    });
  });

  it('filters spores by content type and shows an empty filtered state when nothing matches', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('2 shown / 5 total')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter spores by content type'), {
      target: { value: 'image' },
    });

    await waitFor(() => {
      expect(screen.getByText('1 shown / 5 total')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter spores by content type'), {
      target: { value: 'audio' },
    });

    await waitFor(() => {
      // Gallery panel shows "No objects in this collection" when filter results in 0 items
      expect(screen.getByText('No objects in this collection')).toBeInTheDocument();
      expect(screen.getByText('0 shown / 5 total')).toBeInTheDocument();
    });
  });

  it('filters spores by search keyword', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Search spores')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Search spores'), {
      target: { value: 'text/plain' },
    });

    await waitFor(() => {
      expect(screen.getByText('1 shown / 5 total')).toBeInTheDocument();
      expect(screen.getAllByText('text/plain').length).toBeGreaterThan(0);
    });
  });

  it('hydrates list controls from URL search params', async () => {
    mockSearchParamsString = 'content=text&sort=sizeAsc&q=text%2Fplain';
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Filter spores by content type')).toHaveValue('text');
      expect(screen.getByLabelText('Search spores')).toHaveValue('text/plain');
      expect(screen.getByLabelText('Sort spores')).toHaveValue('sizeAsc');
      expect(screen.getByText('1 shown / 5 total')).toBeInTheDocument();
    });
  });

  it('updates URL search params when list controls change', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Filter spores by content type')).toBeInTheDocument();
      expect(screen.getByLabelText('Sort spores')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter spores by content type'), {
      target: { value: 'image' },
    });
    fireEvent.change(screen.getByLabelText('Sort spores'), {
      target: { value: 'sizeDesc' },
    });
    fireEvent.change(screen.getByLabelText('Search spores'), {
      target: { value: '0x2222' },
    });

    await waitFor(() => {
      expect(
        mockReplace.mock.calls.some(
          ([href]) =>
            String(href).includes('content=image') &&
            String(href).includes('sort=sizeDesc') &&
            String(href).includes('q=0x2222')
        )
      ).toBe(true);
    });
  });

  it('updates tab query param when switching collection tabs', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Holders \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^Holders \(/ }));

    await waitFor(() => {
      expect(screen.getByText('No holders in this collection')).toBeInTheDocument();
      expect(mockReplace.mock.calls.some(([href]) => String(href).includes('tab=holders'))).toBe(
        true
      );
    });
    expect(api.getSporeClusterHolders).toHaveBeenCalledWith(
      mockClusterId,
      expect.objectContaining({ limit: 50 })
    );
  });

  it('renders cluster description JSON with metadata blocks', async () => {
    const jsonDescriptionCluster = {
      ...mockCluster,
      description: JSON.stringify({
        description: 'On-chain generative spores',
        version: 1,
        category: 'collectible',
        dob: {
          ver: 1,
          pattern: [{}, {}],
          decoders: [{}],
        },
      }),
    };
    vi.mocked(api.getSporeCluster).mockResolvedValue(jsonDescriptionCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('On-chain generative spores')).toBeInTheDocument();
      // Non-DOB metadata still shows in description area
      const versionLabel = screen.getByText('Version');
      expect(versionLabel).toBeInTheDocument();
      expect(versionLabel.parentElement?.textContent).toContain('1');
      expect(screen.getByText('Category')).toBeInTheDocument();
      expect(screen.getByText('collectible')).toBeInTheDocument();
      // DOB metadata moves to the DOB Blueprint section
      expect(screen.getByText('DOB Blueprint')).toBeInTheDocument();
      expect(screen.getByText('version')).toBeInTheDocument();
      expect(screen.getByText('traits')).toBeInTheDocument();
      expect(screen.getByText('View Raw Cluster Metadata JSON')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('View Raw Cluster Metadata JSON'));

    await waitFor(() => {
      // Raw JSON is now syntax-highlighted (split across spans), so check container text
      const pre = screen
        .getByText('View Raw Cluster Metadata JSON')
        .closest('details')
        ?.querySelector('pre');
      expect(pre).toBeInTheDocument();
      expect(pre?.textContent).toContain('"category"');
      expect(pre?.textContent).toContain('"collectible"');
    });
  });

  it('resolves creator address from owner lock hash', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue({
      ...mockCluster,
      ownerAddress: undefined,
    });
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(api.getAddress).toHaveBeenCalledWith(mockCluster.ownerLockHash);
      const ownerLink = screen.getByRole('link', {
        name: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      });
      expect(ownerLink).toBeInTheDocument();
      expect(ownerLink).toHaveAttribute(
        'href',
        '/mainnet/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3'
      );
    });
  });

  it('resolves spore owner address from owner lock hash', async () => {
    const resolvedSporeOwnerAddress = 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgpz9p9j';
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);
    vi.mocked(api.getAddress).mockImplementation(async (addr: string) => {
      if (addr === mockSpores.data[1].ownerLockHash) {
        return {
          lockScriptHash: addr,
          address: resolvedSporeOwnerAddress,
          balance: '0',
          commonKnowledgeSize: '0',
          liveCellsCount: 0,
          transactionsCount: 0,
        } as any;
      }
      return {
        lockScriptHash: addr,
        address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
        balance: '0',
        commonKnowledgeSize: '0',
        liveCellsCount: 0,
        transactionsCount: 0,
      } as any;
    });

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      // Gallery panel is always visible, spore cards show resolved addresses
      expect(api.getAddress).toHaveBeenCalledWith(mockSpores.data[1].ownerLockHash);
      const addressLinks = screen.getAllByRole('link');
      expect(
        addressLinks.find(
          (link) => link.getAttribute('href') === `/mainnet/address/${resolvedSporeOwnerAddress}`
        )
      ).toBeTruthy();
      expect(
        addressLinks.find(
          (link) =>
            link.getAttribute('href') === `/mainnet/address/${mockSpores.data[1].ownerLockHash}`
        )
      ).toBeUndefined();
    });
  });

  it('handles unnamed collection', async () => {
    const unnamedCluster = { ...mockCluster, name: null };
    vi.mocked(api.getSporeCluster).mockResolvedValue(unnamedCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Unnamed Collection')).toBeInTheDocument();
    });
  });

  it('renders pagination status in gallery panel', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      // Gallery panel uses GALLERY_PAGE_SIZE (18), label is "Objects"
      expect(screen.getByText('Showing 1-2 of 5 Objects, 18 per page')).toBeInTheDocument();
      // Both gallery panel and activities tab render pagination, so multiple
      // Previous/Next buttons exist; verify the gallery-specific pagination text
      const paginationButtons = screen.getAllByRole('button', { name: 'Previous' });
      expect(paginationButtons.length).toBeGreaterThanOrEqual(1);
      expect(paginationButtons.some((btn) => (btn as HTMLButtonElement).disabled)).toBe(true);
    });
  });
});
