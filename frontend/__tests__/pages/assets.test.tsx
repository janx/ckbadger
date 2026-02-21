import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import AssetsPage from '@/app/assets/page';
import { api } from '@/lib/api';

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
      assetType: 'dob' as const,
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
      totalSupply: null,
      contentType: null,
      contentSize: null,
      clusterId: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
      clusterName: 'Test Collection',
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
    window.history.replaceState(null, '', '/assets');
    vi.mocked(api.getToken).mockImplementation(
      async () => ({ totalOccupiedCapacity: '0', totalCapacity: '0' }) as any
    );
    vi.mocked(api.getNftCollection).mockImplementation(
      async () => ({ liveOccupiedCapacity: '0', liveCapacity: '0' }) as any
    );
    vi.mocked(api.getSporeCluster).mockImplementation(
      async () => ({ liveOccupiedCapacity: '0', liveCapacity: '0' }) as any
    );
  });

  it('renders the page with header and title', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Assets')).toBeInTheDocument();
  });

  it('renders three tabs: Tokens, NFTs, DOBs', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    expect(screen.getByRole('button', { name: /Tokens/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /NFTs/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /DOBs/i })).toBeInTheDocument();
  });

  it('shows Tokens tab content by default', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'token' }));
    });
  });

  it('uses query type as initial tab', async () => {
    window.history.replaceState(null, '', '/assets?type=dob');
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'dob' }));
      expect(screen.getByText('DOB Collections')).toBeInTheDocument();
    });
  });

  it('updates query type when tab changes', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    fireEvent.click(screen.getByRole('button', { name: /NFTs/i }));

    await waitFor(() => {
      expect(window.location.search).toBe('?type=nft');
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'nft' }));
    });
  });

  it('switches to DOBs tab when clicked', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const dobsTab = screen.getByRole('button', { name: /DOBs/i });
    fireEvent.click(dobsTab);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'dob' }));
    });
  });

  it('shows DOBs info banner when DOBs tab is active', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const dobsTab = screen.getByRole('button', { name: /DOBs/i });
    fireEvent.click(dobsTab);

    await waitFor(() => {
      expect(screen.getByText('DOB Collections')).toBeInTheDocument();
    });
  });

  it('shows NFTs info banner when NFTs tab is active', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    const nftsTab = screen.getByRole('button', { name: /NFTs/i });
    fireEvent.click(nftsTab);

    await waitFor(() => {
      expect(screen.getByText('NFT Collections')).toBeInTheDocument();
    });
  });

  it('displays token data in the table', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByText('TEST')).toBeInTheDocument();
      expect(screen.getByText('XUDT')).toBeInTheDocument();
    });
  });

  it('displays cluster data in DOBs tab', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const dobsTab = screen.getByRole('button', { name: /DOBs/i });
    fireEvent.click(dobsTab);

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

  it('shows ON-CHAIN badge for DOB assets', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const dobsTab = screen.getByRole('button', { name: /DOBs/i });
    fireEvent.click(dobsTab);

    await waitFor(() => {
      expect(screen.getAllByText('ON-CHAIN').length).toBeGreaterThan(0);
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
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    vi.mocked(api.getToken).mockImplementation(async (typeHash: string) => {
      const totalOccupiedCapacity =
        typeHash === sortableTokenAssets.data[0].id ? '1000000000' : '5000000000';
      const totalCapacity =
        typeHash === sortableTokenAssets.data[0].id ? '2000000000' : '9000000000';
      return { totalOccupiedCapacity, totalCapacity } as any;
    });

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Occupied' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Occupied' }));

    await waitFor(() => {
      const tokenLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/tokens/'));
      expect(tokenLinks[0]).toHaveTextContent('BETA');
    });
  });

  it('defaults to sorting by capacity in descending order', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    vi.mocked(api.getToken).mockImplementation(async (typeHash: string) => {
      const totalCapacity =
        typeHash === sortableTokenAssets.data[0].id ? '2000000000' : '9000000000';
      return { totalOccupiedCapacity: '0', totalCapacity } as any;
    });

    render(<AssetsPage />);

    await waitFor(() => {
      const tokenLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/tokens/'));
      expect(tokenLinks[0]).toHaveTextContent('BETA');
    });
  });

  it('supports sorting by capacity', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    vi.mocked(api.getToken).mockImplementation(async (typeHash: string) => {
      if (typeHash === sortableTokenAssets.data[0].id) {
        return { totalOccupiedCapacity: '1000000000', totalCapacity: '8000000000' } as any;
      }
      return { totalOccupiedCapacity: '5000000000', totalCapacity: '2000000000' } as any;
    });

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Capacity' }));

    await waitFor(() => {
      const tokenLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/tokens/'));
      expect(tokenLinks[0]).toHaveTextContent('BETA');
    });
  });
});
