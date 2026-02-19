import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ClusterDetailPage from '@/app/clusters/[clusterId]/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeCluster: vi.fn(),
    getSporesByCluster: vi.fn(),
    getSporeClusterOccupationChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockClusterId = '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890';

vi.mock('next/navigation', () => ({
  useParams: () => ({ clusterId: mockClusterId }),
  useRouter: () => ({ push: vi.fn() }),
}));

const mockCluster = {
  clusterId: mockClusterId,
  name: 'Test Collection',
  description: 'A test collection of DOBs',
  ownerLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
  sporesCount: 5,
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
    vi.mocked(api.getSporeClusterOccupationChart).mockResolvedValue({
      title: 'Test Collection Capacity Occupation',
      data: [],
      series: [],
    });
  });

  it('renders the page with header', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
  });

  it('shows loading state initially', async () => {
    vi.mocked(api.getSporeCluster).mockImplementation(() => new Promise(() => {}));
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    const skeletons = document.querySelectorAll('.animate-pulse');
    expect(skeletons.length).toBeGreaterThan(0);
  });

  it('displays cluster name and info', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Test Collection')).toBeInTheDocument();
      expect(screen.getByText('A test collection of DOBs')).toBeInTheDocument();
    });
  });

  it('shows Spore Cluster badge', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Spore Cluster')).toBeInTheDocument();
    });
  });

  it('renders occupation history panel', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Capacity & Occupation')).toBeInTheDocument();
    });
  });

  it('displays total DOBs count', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Total DOBs')).toBeInTheDocument();
      const dobsValue = document.querySelector('.text-amber.text-xl');
      expect(dobsValue?.textContent).toBe('5');
    });
  });

  it('displays DOBs table with content', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('image/png')).toBeInTheDocument();
      expect(screen.getByText('text/plain')).toBeInTheDocument();
    });
  });

  it('shows content size for each DOB', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('1,024 B')).toBeInTheDocument();
      expect(screen.getByText('256 B')).toBeInTheDocument();
    });
  });

  it('shows error state when cluster not found', async () => {
    vi.mocked(api.getSporeCluster).mockRejectedValue(new Error('Not found'));
    vi.mocked(api.getSporesByCluster).mockResolvedValue(emptySpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Spore Cluster not found')).toBeInTheDocument();
    });
  });

  it('shows empty state when cluster has no DOBs', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(emptySpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('No DOBs in this collection')).toBeInTheDocument();
    });
  });

  it('displays back to DOBs link', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      const backLink = screen.getByText('← Back to DOBs');
      expect(backLink).toBeInTheDocument();
      expect(backLink.closest('a')).toHaveAttribute('href', '/assets?type=dob');
    });
  });

  it('shows cluster info section with all fields', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Cluster Info')).toBeInTheDocument();
      expect(screen.getByText('Cluster ID')).toBeInTheDocument();
      expect(screen.getByText('Description')).toBeInTheDocument();
      expect(screen.getByText('Total DOBs')).toBeInTheDocument();
      expect(screen.getByText('Creator')).toBeInTheDocument();
      expect(screen.getByText('Created at Block')).toBeInTheDocument();
    });
  });

  it('handles unnamed collection', async () => {
    const unnamedCluster = { ...mockCluster, name: null };
    vi.mocked(api.getSporeCluster).mockResolvedValue(unnamedCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Unnamed Collection')).toBeInTheDocument();
    });
  });

  it('shows pagination buttons', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Previous' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Next' })).toBeInTheDocument();
    });
  });

  it('disables Previous button on first page', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
    });
  });

  it('disables Next button when no more pages', async () => {
    vi.mocked(api.getSporeCluster).mockResolvedValue(mockCluster);
    vi.mocked(api.getSporesByCluster).mockResolvedValue(mockSpores);

    render(<ClusterDetailPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    });
  });
});
