import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ClusterDetailPage from '@/app/clusters/[clusterId]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeCluster: vi.fn(),
    getSporesByCluster: vi.fn(),
    getSporeClusterOccupationChart: vi.fn(),
    getSporeClusterHolders: vi.fn(),
    getSporeClusterActivities: vi.fn(),
    getAddress: vi.fn(),
  },
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
  ownerLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
  sporesCount: 5,
  holdersCount: 3,
  activitiesCount: 10,
  createdAtBlock: 1000000,
  liveCapacity: '100000000000',
  liveOccupiedCapacity: '61000000000',
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
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const emptySpores = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('ClusterDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSearchParamsString = '';
    vi.mocked(api.getSporeClusterOccupationChart).mockResolvedValue({
      title: 'Test Collection Capacity Occupation',
      data: [],
      series: [],
    });
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: mockCluster.ownerLockHash,
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      occupiedCapacity: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getSporeClusterHolders).mockResolvedValue({
      data: [],
      total: 0,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getSporeClusterActivities).mockResolvedValue({
      data: [],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders the page with header', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
  });

  it('shows loading state initially', async () => {
    vi.mocked(api.getSporeCluster).mockImplementation(() => new Promise(() => {}));
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    const skeletons = document.querySelectorAll('.animate-pulse');
    expect(skeletons.length).toBeGreaterThan(0);
  });

  it('displays cluster name and info', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Test Collection')).toBeInTheDocument();
      expect(screen.getByText('A test collection of spores')).toBeInTheDocument();
    });
  });

  it('shows Spore Cluster badge', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Spore Cluster')).toBeInTheDocument();
    });
  });

  it('renders occupation history panel', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Capacity & Occupation')).toBeInTheDocument();
    });
  });

  it('displays total spores count', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getAllByText('Total Spores').length).toBeGreaterThan(0);
      const sporesValue = document.querySelector('.text-warning.text-xl');
      expect(sporesValue?.textContent).toBe('5');
    });
  });

  it('displays spores table with content', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
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
          sporeId: '',
          contentType: undefined,
          ownerLockHash: undefined,
          ownerAddress: undefined,
          contentSize: undefined,
          createdAtBlock: undefined,
        } as any,
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getAllByText('Unknown spore ID').length).toBeGreaterThan(0);
      expect(screen.getAllByText('unknown').length).toBeGreaterThan(0);
    });
  });

  it('renders collection tabs with Activities active by default', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Activities \(/ })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /^Holders \(/ })).toBeInTheDocument();
      expect(screen.queryByLabelText('Search spores')).not.toBeInTheDocument();
    });
  });

  it('hydrates collection tab from query params', async () => {
    mockSearchParamsString = 'tab=nfts';
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Search spores')).toBeInTheDocument();
      expect(screen.queryByText('No activities in this collection')).not.toBeInTheDocument();
    });
  });

  it('falls back to Activities tab when tab query is invalid', async () => {
    mockSearchParamsString = 'tab=invalid';
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('No activities in this collection')).toBeInTheDocument();
    });
  });

  it('shows content size for each spore', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getAllByText('1,024 B').length).toBeGreaterThan(0);
      expect(screen.getAllByText('256 B').length).toBeGreaterThan(0);
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
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByText('No spores in this collection')).toBeInTheDocument();
    });
  });

  it('displays back to NFTs link', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      const backLink = screen.getByText('← Back to NFTs');
      expect(backLink).toBeInTheDocument();
      expect(backLink.closest('a')).toHaveAttribute('href', '/assets?type=nft');
    });
  });

  it('shows cluster info section with all fields', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Cluster Info')).toBeInTheDocument();
      expect(screen.getByText('Cluster ID')).toBeInTheDocument();
      expect(screen.getByText('Description')).toBeInTheDocument();
      expect(screen.getAllByText('Total Spores').length).toBeGreaterThan(0);
      expect(screen.getByText('Creator')).toBeInTheDocument();
      expect(screen.queryByText('Created at Block')).not.toBeInTheDocument();
    });

    const clusterIdField = screen.getByText('Cluster ID').closest('div')?.parentElement;
    const creatorField = screen.getByText('Creator').closest('div')?.parentElement;
    expect(clusterIdField).toHaveClass('flex-col');
    expect(creatorField).toHaveClass('flex-col');
  });

  it('renders overview and snapshot sections', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByText('Live Capacity')).toBeInTheDocument();
      expect(screen.getByText('Occupied Capacity')).toBeInTheDocument();
      expect(screen.getByText('Content Snapshot (Filtered View)')).toBeInTheDocument();
      expect(screen.getByText('Average Payload Size')).toBeInTheDocument();
      expect(screen.getByText('image')).toBeInTheDocument();
      expect(screen.getByText('text')).toBeInTheDocument();
    });
  });

  it('filters spores by content type', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByText('2 shown / 5 total')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter spores by content type'), {
      target: { value: 'image' },
    });

    await waitFor(() => {
      expect(screen.getByText('1 shown / 5 total')).toBeInTheDocument();
      expect(screen.queryByText('No spores match current filters')).not.toBeInTheDocument();
    });
  });

  it('uses wrapped control layout for narrow screens in spores panel', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByLabelText('Search spores')).toBeInTheDocument();
    });

    const searchInput = screen.getByLabelText('Search spores');
    expect(searchInput.className).toContain('w-full');
    expect(searchInput.className).toContain('sm:w-48');
  });

  it('shows empty filtered state when no spores match selected type', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByLabelText('Filter spores by content type')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter spores by content type'), {
      target: { value: 'audio' },
    });

    await waitFor(() => {
      expect(screen.getByText('No spores match current filters')).toBeInTheDocument();
      expect(screen.getByText('0 shown / 5 total')).toBeInTheDocument();
    });
  });

  it('filters spores by search keyword', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByLabelText('Search spores')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Search spores'), {
      target: { value: 'text/plain' },
    });

    await waitFor(() => {
      expect(screen.getByText('1 shown / 5 total')).toBeInTheDocument();
      expect(screen.getAllByText('text/plain').length).toBeGreaterThan(0);
      expect(screen.queryByText('No spores match current filters')).not.toBeInTheDocument();
    });
  });

  it('hydrates list controls from URL search params', async () => {
    mockSearchParamsString = 'tab=nfts&content=text&sort=sizeAsc&q=text%2Fplain';
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Filter spores by content type')).toHaveValue('text');
      expect(screen.getByLabelText('Search spores')).toHaveValue('text/plain');
      expect(screen.getByText('1 shown / 5 total')).toBeInTheDocument();
    });
  });

  it('uses sortable column headers instead of sort dropdown', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.queryByLabelText('Sort spores')).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort spores by size' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort spores by block' })).toBeInTheDocument();
    });
  });

  it('keeps NFT controls to the left of tab names', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByTestId('spore-list-controls')).toBeInTheDocument();
    });

    const controls = screen.getByTestId('spore-list-controls');
    const tabsList = screen.getByRole('button', { name: /^Activities \(/ }).parentElement;
    expect(tabsList).not.toBeNull();
    expect(controls.compareDocumentPosition(tabsList!) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
  });

  it('updates URL search params when list controls and sortable headers change', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByLabelText('Filter spores by content type')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter spores by content type'), {
      target: { value: 'image' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sort spores by size' }));
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
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(mockReplace.mock.calls.some(([href]) => String(href).includes('tab=nfts'))).toBe(true);
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
      expect.objectContaining({ limit: 20 })
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
      const versionLabel = screen.getByText('Version');
      expect(versionLabel).toBeInTheDocument();
      expect(versionLabel.parentElement?.textContent).toContain('1');
      expect(screen.getByText('Category')).toBeInTheDocument();
      expect(screen.getByText('collectible')).toBeInTheDocument();
      expect(screen.getByText('DOB Version')).toBeInTheDocument();
      expect(screen.getByText('DOB Pattern Items')).toBeInTheDocument();
      expect(screen.getByText('DOB Decoders')).toBeInTheDocument();
      expect(screen.getByText('View Raw Cluster Metadata JSON')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('View Raw Cluster Metadata JSON'));

    await waitFor(() => {
      expect(screen.getByText(/"category": "collectible"/)).toBeInTheDocument();
      expect(screen.getByText(/"description": "On-chain generative spores"/)).toBeInTheDocument();
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
        '/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3'
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
          occupiedCapacity: '0',
          liveCellsCount: 0,
          transactionsCount: 0,
        } as any;
      }
      return {
        lockScriptHash: addr,
        address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
        balance: '0',
        occupiedCapacity: '0',
        liveCellsCount: 0,
        transactionsCount: 0,
      } as any;
    });

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(api.getAddress).toHaveBeenCalledWith(mockSpores.data[1].ownerLockHash);
      expect(
        document.querySelector(`a[href="/address/${resolvedSporeOwnerAddress}"]`)
      ).toBeInTheDocument();
      expect(
        document.querySelector(`a[href="/address/${mockSpores.data[1].ownerLockHash}"]`)
      ).not.toBeInTheDocument();
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

  it('shows pagination buttons', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByText('Showing 1-2 of 5 Spores, 20 per page')).toBeInTheDocument();
      expect(screen.getByText('Page 1 of 1')).toBeInTheDocument();
    });
  });

  it('disables Previous button on first page', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
    });
  });

  it('disables Next button when no more pages', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage clusterId={mockClusterId} />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^NFTs \(/ })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^NFTs \(/ }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    });
  });
});
