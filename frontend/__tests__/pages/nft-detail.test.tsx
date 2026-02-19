import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import SporeDetailPage from '@/app/nfts/[sporeId]/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getSporeNft: vi.fn(),
    getSporeCluster: vi.fn(),
    getSporeNftOccupationChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('next/navigation', () => ({
  useParams: () => ({
    sporeId: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
  }),
}));

const mockSpore = {
  sporeId: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
  txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  outputIndex: 0,
  clusterId: null,
  contentType: 'image/png',
  contentSize: 1024,
  ownerLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  isLive: true,
  createdAtBlock: 123456,
  liveCapacity: '100000000000',
  liveOccupiedCapacity: '61000000000',
};

describe('SporeDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getSporeNftOccupationChart).mockResolvedValue({
      title: 'Spore Capacity Occupation',
      data: [],
      series: [],
    });
  });

  it('links back to NFT tab on assets page', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      const backLink = screen.getByText('← Back to NFTs');
      expect(backLink).toBeInTheDocument();
      expect(backLink.closest('a')).toHaveAttribute('href', '/assets?type=nft');
    });
  });

  it('renders occupation history panel', async () => {
    vi.mocked(api.getSporeNft).mockResolvedValue(mockSpore);

    render(<SporeDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Occupation History')).toBeInTheDocument();
    });
  });
});
