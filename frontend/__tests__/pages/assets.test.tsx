import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import AssetsPage from '@/app/assets/page';
import { api } from '@/lib/api';

const mockReplace = vi.fn((href: string) => {
  const url = new URL(href, 'http://localhost');
  window.history.replaceState(null, '', `${url.pathname}${url.search}`);
});

vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(window.location.search),
  usePathname: () => '/assets',
  useRouter: () => ({ replace: mockReplace }),
}));

vi.mock('@/lib/api', () => ({
  api: {
    getAssets: vi.fn(),
    getToken: vi.fn(),
    getObjectCollection: vi.fn(),
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
      hMultiplier: 2.0,
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
      assetType: 'object' as const,
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
      storageTier: 'fully_onchain' as const,
      fullyOnchainRatio: '1.0000',
      fullyOnchainCount: 50,
      hMultiplier: 1.8,
    },
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const mockDotbitIdentityAssets = {
  data: [
    {
      id: '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f',
      assetType: 'identity' as const,
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
      hMultiplier: 2.33,
    },
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const mockDidCkbIdentityAssets = {
  data: [
    {
      id: '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f',
      assetType: 'identity' as const,
      standard: 'did_ckb',
      name: 'did:ckb',
      symbol: null,
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 6400,
      transfersCount: 32000,
      transfers24h: 120,
      decimals: null,
      totalSupply: '420000',
      contentType: null,
      contentSize: null,
      clusterId: null,
      clusterName: null,
      liveCapacity: '8000000000',
      liveOccupiedCapacity: '5000000000',
      hMultiplier: 1.6,
    },
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const mockMixedObjectAssets = {
  data: [
    {
      id: '0x1111111111111111111111111111111111111111111111111111111111111111',
      assetType: 'object' as const,
      standard: 'spore',
      name: 'Spore With Icon',
      symbol: null,
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 10,
      transfersCount: 12,
      transfers24h: 1,
      decimals: null,
      totalSupply: '10',
      contentType: null,
      contentSize: null,
      clusterId: '0x1111111111111111111111111111111111111111111111111111111111111111',
      clusterName: 'Spore With Icon',
      liveCapacity: '3000000000',
      liveOccupiedCapacity: '1000000000',
      storageTier: 'fully_onchain' as const,
      fullyOnchainRatio: '1.0000',
      fullyOnchainCount: 10,
      hMultiplier: 3.0,
    },
    {
      id: '0x2222222222222222222222222222222222222222222222222222222222222222',
      assetType: 'object' as const,
      standard: 'm-nft',
      name: 'MNFT Without Icon',
      symbol: null,
      iconUrl: null,
      published: false,
      famous: false,
      tags: null,
      holdersCount: 8,
      transfersCount: 9,
      transfers24h: 0,
      decimals: null,
      totalSupply: '8',
      contentType: null,
      contentSize: null,
      clusterId: null,
      clusterName: null,
      liveCapacity: '2000000000',
      liveOccupiedCapacity: '900000000',
      storageTier: 'centralized_dependent' as const,
      fullyOnchainRatio: '0.0000',
      fullyOnchainCount: 0,
      hMultiplier: 2.22,
    },
  ],
  total: 2,
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
      hMultiplier: 2.0,
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
      hMultiplier: 1.8,
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

  it('renders three tabs: Tokens, Objects, and Identities', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    expect(screen.getByRole('button', { name: /Tokens/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Objects/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Identities/i })).toBeInTheDocument();
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
      // Dual-render: name appears in both table row and card layout
      expect(screen.getAllByText(fallbackLabel).length).toBeGreaterThanOrEqual(1);
    });
  });

  it('uses query type as initial tab', async () => {
    window.history.replaceState(null, '', '/assets?type=object');
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'object' }));
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('maps legacy dob query type to object', async () => {
    window.history.replaceState(null, '', '/assets?type=dob');
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'object' }));
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('maps legacy nft query type to object', async () => {
    window.history.replaceState(null, '', '/assets?type=nft');
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'object' }));
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('updates query type when tab changes', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(window.location.search).toBe('?type=object');
      expect(mockReplace).toHaveBeenCalledWith('/assets?type=object', { scroll: false });
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'object' }));
    });
  });

  it('does not show info banner when Objects tab is active', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    const objectsTab = screen.getByRole('button', { name: /Objects/i });
    fireEvent.click(objectsTab);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'object' }));
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
      // Dual-render: text appears in both table and card layouts
      expect(screen.getAllByText('TEST').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('xUDT').length).toBeGreaterThan(0);
    });
  });

  it('displays spore collection data in Objects tab', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const objectsTab = screen.getByRole('button', { name: /Objects/i });
    fireEvent.click(objectsTab);

    await waitFor(() => {
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('SPORE').length).toBeGreaterThan(0);
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

  it('filters assets by selected standard', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'token', standard: undefined })
      );
    });

    fireEvent.change(screen.getByLabelText('Filter by standard'), {
      target: { value: 'xudt' },
    });

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'token', standard: 'xudt' })
      );
      expect(window.location.search).toBe('?standard=xudt');
    });
  });

  it('shows did:ckb in Identity standards and removes D-ID option', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockDidCkbIdentityAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Identities/i }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'identity' }));
    });

    const standardFilter = screen.getByLabelText('Filter by standard');
    expect(screen.getByRole('option', { name: 'did:ckb' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'D-ID' })).not.toBeInTheDocument();

    fireEvent.change(standardFilter, { target: { value: 'did:ckb' } });
    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'identity', standard: 'did:ckb' })
      );
      expect(window.location.search).toContain('standard=did%3Ackb');
    });
  });

  it('clears search when switching tabs', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    const searchInput = screen.getByPlaceholderText('Search by name...');
    fireEvent.change(searchInput, { target: { value: 'test' } });

    const searchButton = screen.getByRole('button', { name: 'Search' });
    fireEvent.click(searchButton);

    const objectsTab = screen.getByRole('button', { name: /Objects/i });
    fireEvent.click(objectsTab);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: undefined })
      );
    });
  });

  it('clears standard filter when switching tabs', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    fireEvent.change(screen.getByLabelText('Filter by standard'), {
      target: { value: 'xudt' },
    });

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'token', standard: 'xudt' })
      );
    });

    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(window.location.search).toBe('?type=object');
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', standard: undefined })
      );
    });
  });

  it('routes spore collections to cluster detail page', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const objectsTab = screen.getByRole('button', { name: /Objects/i });
    fireEvent.click(objectsTab);

    await waitFor(() => {
      // Dual-render: links appear in both table and card layouts
      const links = screen.getAllByRole('link', { name: /Test Collection/i });
      expect(links[0]).toHaveAttribute(
        'href',
        '/clusters/0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890'
      );
    });
  });

  it('uses dotbit slug for dotbit identity detail links', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockDotbitIdentityAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Identities/i }));

    await waitFor(() => {
      // Dual-render: links appear in both table and card layouts
      const links = screen.getAllByRole('link', { name: /\.bit/i });
      expect(links[0]).toHaveAttribute(
        'href',
        '/identities/dotbit/0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f'
      );
      expect(screen.getAllByText('DOTBIT').length).toBeGreaterThan(0);
    });
  });

  it('uses did:ckb slug for did:ckb identity detail links', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockDidCkbIdentityAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Identities/i }));

    await waitFor(() => {
      // Dual-render: links appear in both table and card layouts
      const links = screen.getAllByRole('link', { name: /did:ckb/i });
      expect(links[0]).toHaveAttribute(
        'href',
        '/identities/did/0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f'
      );
      expect(screen.getAllByText('did:ckb').length).toBeGreaterThan(0);
    });
  });

  it('keeps collection names aligned by rendering a fixed icon slot for every object row', async () => {
    vi.mocked(api.getAssets).mockImplementation((params) => {
      const assetType = params?.type;
      return Promise.resolve(assetType === 'object' ? mockMixedObjectAssets : emptyAssets);
    });

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      // Dual-render: names appear in both table and card layouts
      expect(screen.getAllByText('Spore With Icon').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('MNFT Without Icon').length).toBeGreaterThanOrEqual(1);
    });

    // Dual-render: each asset has icon slots in both table (lg+) and card (<lg) layouts
    const iconSlots = screen.getAllByTestId('asset-icon-slot');
    expect(iconSlots).toHaveLength(4); // 2 assets x 2 layouts
    // Order: [0]=table-spore, [1]=card-spore, [2]=table-mnft, [3]=card-mnft
    expect(iconSlots[0]).toHaveTextContent('🗂️');
    expect(iconSlots[2].textContent?.trim()).toBe('');
  });

  it('shows verified badge for published tokens', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      // Dual-render: verified badge appears in both table and card layouts
      const verifiedIcons = document.querySelectorAll('[title="Verified"]');
      expect(verifiedIcons.length).toBeGreaterThanOrEqual(1);
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
      expect(screen.getByRole('button', { name: 'Sort by Occupied (CKB)' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity (CKB)' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Occupied (CKB)' }));

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
      expect(screen.getByRole('button', { name: 'Sort by Capacity (CKB)' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Capacity (CKB)' }));

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

  it('filters object assets by storage tier', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', storageTier: undefined })
      );
    });

    fireEvent.change(screen.getByLabelText('Filter by storage tier'), {
      target: { value: 'fully_onchain' },
    });

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', storageTier: 'fully_onchain' })
      );
      expect(window.location.search).toContain('storageTier=fully_onchain');
    });
  });

  it('maps merged Offchain Dependent storage filter to API query', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', storageTier: undefined })
      );
    });

    fireEvent.change(screen.getByLabelText('Filter by storage tier'), {
      target: { value: 'offchain_dependent' },
    });

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', storageTier: 'offchain_dependent' })
      );
      expect(window.location.search).toContain('storageTier=offchain_dependent');
    });
  });

  it('shows storage badge for object assets', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      // Dual-render: storage badge appears in both table and card layouts
      expect(screen.getAllByText('FULLY ON-CHAIN').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('uses updated table headers and removes on-chain/transfers columns', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Standard' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Occupied (CKB)' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity (CKB)' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Sort by Transfers' })).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Sort by On-chain' })).not.toBeInTheDocument();
    });
  });

  it('renders merged offchain storage badge for decentralized/centralized tiers', async () => {
    vi.mocked(api.getAssets).mockImplementation((params) => {
      const assetType = params?.type;
      return Promise.resolve(assetType === 'object' ? mockMixedObjectAssets : emptyAssets);
    });

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      // Dual-render: text appears in both table and card layouts
      expect(screen.getAllByText('MNFT Without Icon').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('OFFCHAIN DEPENDENT').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('renders HM sort header at xl width', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    render(<AssetsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by HM' })).toBeInTheDocument();
    });
  });

  it('renders HM column with formatted multiplier value', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    render(<AssetsPage />);
    await waitFor(() => {
      // mockTokenAssets.data[0].hMultiplier is 2.0
      expect(screen.getByText('×2.00')).toBeInTheDocument();
    });
  });

  it('renders Circulation sort header for tokens tab at xl width', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    render(<AssetsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Circulation' })).toBeInTheDocument();
    });
  });

  it('supports sorting by HM', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    render(<AssetsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by HM' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sort by HM' }));
    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'hMultiplier', sortDirection: 'desc' })
      );
    });
  });

  it('renders object table with dual-render layout (table lg+ and card below lg)', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);
    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      // Dual-render: name appears in both table and card layouts
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
    });

    // Should have both table and card layouts rendered (dual-render)
    const iconSlots = screen.getAllByTestId('asset-icon-slot');
    // Each asset renders two icon slots: one for lg+ table, one for <lg card
    expect(iconSlots.length).toBeGreaterThanOrEqual(2);
  });
});
