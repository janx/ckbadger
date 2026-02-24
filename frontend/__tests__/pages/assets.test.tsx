import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import AssetsPage from '@/app/assets/page';
import { api } from '@/lib/api';

const mockReplace = vi.fn((href: string) => {
  const url = new URL(href, 'http://localhost');
  window.history.replaceState(null, '', `${url.pathname}${url.search}`);
});

vi.mock('next/navigation', () => ({
  useSearchParams: () => new URLSearchParams(window.location.search),
  usePathname: () => '/assets',
  useRouter: () => ({ replace: mockReplace }),
}));

vi.mock('@/lib/api', () => ({
  api: {
    getAssets: vi.fn(),
    getToken: vi.fn(),
    getNftCollection: vi.fn(),
    getSporeCluster: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockTokenAssets = {
  data: [
    {
      id: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
      assetType: 'token' as const,
      standard: 'xudt',
      name: 'Test Token',
      symbol: 'TEST',
      iconUrl: 'https://example.com/icon.png',
      published: true,
      famous: false,
      tags: ['defi'],
      holdersCount: 1000,
      transfersCount: 50000,
      transfers24h: 100,
      decimals: 8,
      totalSupply: '1000000000000',
      contentType: null,
      contentSize: null,
      clusterId: null,
      clusterName: null,
      liveCapacity: '2000000000',
      liveOccupiedCapacity: '1000000000',
    },
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const mockClusterAssets = {
  data: [
    {
      id: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
      assetType: 'nft' as const,
      standard: 'spore',
      name: 'Test Collection',
      symbol: null,
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 50,
      transfersCount: 50,
      transfers24h: 5,
      decimals: null,
      totalSupply: '50',
      contentType: null,
      contentSize: null,
      clusterId: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
      clusterName: 'Test Collection',
      liveCapacity: '9000000000',
      liveOccupiedCapacity: '5000000000',
    },
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const mockDotbitNftAssets = {
  data: [
    {
      id: '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f',
      assetType: 'nft' as const,
      standard: 'dotbit',
      name: '.bit',
      symbol: null,
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 1200,
      transfersCount: 9000,
      transfers24h: 80,
      decimals: null,
      totalSupply: '200000',
      contentType: null,
      contentSize: null,
      clusterId: null,
      clusterName: null,
      liveCapacity: '7000000000',
      liveOccupiedCapacity: '3000000000',
    },
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const emptyAssets = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const sortableTokenAssets = {
  data: [
    {
      id: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      assetType: 'token' as const,
      standard: 'xudt',
      name: 'Alpha Token',
      symbol: 'ALPHA',
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 10,
      transfersCount: 20,
      transfers24h: 2,
      decimals: 8,
      totalSupply: '100000000',
      contentType: null,
      contentSize: null,
      clusterId: null,
      clusterName: null,
      liveCapacity: '2000000000',
      liveOccupiedCapacity: '1000000000',
    },
    {
      id: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      assetType: 'token' as const,
      standard: 'xudt',
      name: 'Beta Token',
      symbol: 'BETA',
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 20,
      transfersCount: 10,
      transfers24h: 1,
      decimals: 8,
      totalSupply: '200000000',
      contentType: null,
      contentSize: null,
      clusterId: null,
      clusterName: null,
      liveCapacity: '9000000000',
      liveOccupiedCapacity: '5000000000',
    },
  ],
  total: 2,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('AssetsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/assets');
  });

  it('renders the page with header and title', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Assets')).toBeInTheDocument();
  });

  it('renders two tabs: Tokens and NFTs', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    expect(screen.getByRole('button', { name: /Tokens/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /NFTs/i })).toBeInTheDocument();
  });

  it('shows Tokens tab content by default', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'token' }));
    });
  });

  it('uses type-hash fallback name for tokens without symbol and name', async () => {
    const typeHash = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
    vi.mocked(api.getAssets).mockResolvedValue({
      ...mockTokenAssets,
      data: [
        {
          ...mockTokenAssets.data[0],
          id: typeHash,
          name: null,
          symbol: null,
        },
      ],
    });

    render(<AssetsPage />);

    const fallbackLabel = `${typeHash.slice(0, 10)}...${typeHash.slice(-8)}`;
    await waitFor(() => {
      expect(screen.getByText(fallbackLabel)).toBeInTheDocument();
    });
  });

  it('uses query type as initial tab', async () => {
    window.history.replaceState(null, '', '/assets?type=nft');
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'nft' }));
      expect(screen.getByText('Test Collection')).toBeInTheDocument();
    });
  });

  it('maps legacy dob query type to nft', async () => {
    window.history.replaceState(null, '', '/assets?type=dob');
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'nft' }));
      expect(screen.getByText('Test Collection')).toBeInTheDocument();
    });
  });

  it('updates query type when tab changes', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    fireEvent.click(screen.getByRole('button', { name: /NFTs/i }));

    await waitFor(() => {
      expect(window.location.search).toBe('?type=nft');
      expect(mockReplace).toHaveBeenCalledWith('/assets?type=nft', { scroll: false });
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'nft' }));
    });
  });

  it('does not show NFTs info banner when NFTs tab is active', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    const nftsTab = screen.getByRole('button', { name: /NFTs/i });
    fireEvent.click(nftsTab);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'nft' }));
    });
    expect(screen.queryByText('NFT Collections')).not.toBeInTheDocument();
    expect(
      screen.queryByText(/includes both standard NFT collections and Spore\/DOB collections/i)
    ).not.toBeInTheDocument();
  });

  it('displays token data in the table', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByText('TEST')).toBeInTheDocument();
      expect(screen.getByText('XUDT')).toBeInTheDocument();
    });
  });

  it('displays spore collection data in NFTs tab', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const nftsTab = screen.getByRole('button', { name: /NFTs/i });
    fireEvent.click(nftsTab);

    await waitFor(() => {
      expect(screen.getByText('Test Collection')).toBeInTheDocument();
      expect(screen.getByText('SPORE')).toBeInTheDocument();
    });
  });

  it('shows loading state', async () => {
    vi.mocked(api.getAssets).mockImplementation(() => new Promise(() => {}));

    render(<AssetsPage />);

    const skeletons = document.querySelectorAll('.animate-pulse');
    expect(skeletons.length).toBeGreaterThan(0);
  });

  it('shows empty state when no assets found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByText('No assets found')).toBeInTheDocument();
    });
  });

  it('handles search functionality', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    const searchInput = screen.getByPlaceholderText('Search by name...');
    fireEvent.change(searchInput, { target: { value: 'test' } });

    const searchButton = screen.getByRole('button', { name: 'Search' });
    fireEvent.click(searchButton);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ search: 'test' }));
    });
  });

  it('clears search when switching tabs', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    const searchInput = screen.getByPlaceholderText('Search by name...');
    fireEvent.change(searchInput, { target: { value: 'test' } });

    const searchButton = screen.getByRole('button', { name: 'Search' });
    fireEvent.click(searchButton);

    const nftsTab = screen.getByRole('button', { name: /NFTs/i });
    fireEvent.click(nftsTab);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: undefined })
      );
    });
  });

  it('routes spore collections to cluster detail page', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const nftsTab = screen.getByRole('button', { name: /NFTs/i });
    fireEvent.click(nftsTab);

    await waitFor(() => {
      const link = screen.getByRole('link', { name: /Test Collection/i });
      expect(link).toHaveAttribute(
        'href',
        '/clusters/0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890'
      );
    });
  });

  it('uses dotbit slug for dotbit NFT detail links', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockDotbitNftAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /NFTs/i }));

    await waitFor(() => {
      const link = screen.getByRole('link', { name: /\.bit/i });
      expect(link).toHaveAttribute('href', '/nfts/dotbit');
      expect(screen.getByText('DOTBIT')).toBeInTheDocument();
    });
  });

  it('shows verified badge for published tokens', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      const verifiedIcon = document.querySelector('[title="Verified"]');
      expect(verifiedIcon).toBeInTheDocument();
    });
  });

  it('renders occupied/capacity columns and supports sorting by occupied', async () => {
    vi.mocked(api.getAssets)
      .mockResolvedValueOnce(sortableTokenAssets)
      .mockResolvedValueOnce({
        ...sortableTokenAssets,
        data: [sortableTokenAssets.data[1], sortableTokenAssets.data[0]],
      });

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Occupied' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Occupied' }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'occupied', sortDirection: 'desc' })
      );
      const tokenLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/tokens/'));
      expect(tokenLinks[0]).toHaveTextContent('BETA');
    });
  });

  it('defaults to sorting by capacity in descending order', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(
        expect.objectContaining({ sortKey: 'capacity', sortDirection: 'desc' })
      );
    });
  });

  it('supports sorting by capacity', async () => {
    vi.mocked(api.getAssets)
      .mockResolvedValueOnce(sortableTokenAssets)
      .mockResolvedValueOnce({
        ...sortableTokenAssets,
        data: [sortableTokenAssets.data[0], sortableTokenAssets.data[1]],
      });

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Capacity' }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'capacity', sortDirection: 'asc' })
      );
      const tokenLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/tokens/'));
      expect(tokenLinks[0]).toHaveTextContent('ALPHA');
    });
  });
});
