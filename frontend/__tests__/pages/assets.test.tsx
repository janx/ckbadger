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
  isWarmupPendingError: vi.fn(() => false),
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
      ownedCapacity: '2000000000',
      ownedKnowledge: '1000000000',
      hMultiplier: 2.0,
    },
  ],
  total: 1,
  limit: 50,
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
      ownedCapacity: '9000000000',
      ownedKnowledge: '5000000000',
      storageTier: 'fully_on_ckb_and_btc' as const,
      fullyOnchainRatio: '1.0000',
      fullyOnchainCount: 50,
      hMultiplier: 1.8,
    },
  ],
  total: 1,
  limit: 50,
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
      ownedCapacity: '7000000000',
      ownedKnowledge: '3000000000',
      hMultiplier: 2.33,
    },
  ],
  total: 1,
  limit: 50,
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
      ownedCapacity: '8000000000',
      ownedKnowledge: '5000000000',
      hMultiplier: 1.6,
    },
  ],
  total: 1,
  limit: 50,
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
      ownedCapacity: '3000000000',
      ownedKnowledge: '1000000000',
      storageTier: 'fully_on_ckb_and_btc' as const,
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
      ownedCapacity: '2000000000',
      ownedKnowledge: '900000000',
      storageTier: 'centralized_dependent' as const,
      fullyOnchainRatio: '0.0000',
      fullyOnchainCount: 0,
      hMultiplier: 2.22,
    },
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

const emptyAssets = {
  data: [],
  total: 0,
  limit: 50,
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
      ownedCapacity: '2000000000',
      ownedKnowledge: '1000000000',
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
      ownedCapacity: '9000000000',
      ownedKnowledge: '5000000000',
      hMultiplier: 1.8,
    },
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('AssetsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/assets');
  });

  it('renders header, tabs, and token content by default', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByTestId('header')).toBeInTheDocument();
      expect(screen.getByText('Assets')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Tokens/i })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Objects/i })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Identities/i })).toBeInTheDocument();
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'token' }));
      expect(screen.getAllByText('TEST').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('xUDT').length).toBeGreaterThan(0);
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

  it('hydrates the Objects tab from current and legacy query types', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    for (const queryType of ['object', 'dob', 'nft']) {
      window.history.replaceState(null, '', `/assets?type=${queryType}`);
      vi.mocked(api.getAssets).mockClear();

      const view = render(<AssetsPage />);

      await waitFor(() => {
        expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'object' }));
        expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
      });

      view.unmount();
    }
  });

  it('updates query type and hides the info banner when Objects tab is active', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(window.location.search).toBe('?type=object');
      expect(mockReplace).toHaveBeenCalledWith('/assets?type=object', { scroll: false });
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'object' }));
      expect(screen.queryByText('NFT Collections')).not.toBeInTheDocument();
      expect(
        screen.queryByText(/includes both standard NFT collections and Spore\/DOB collections/i)
      ).not.toBeInTheDocument();
    });
  });

  it('renders object collection content and cluster link in Objects tab', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<AssetsPage />);

    const objectsTab = screen.getByRole('button', { name: /Objects/i });
    fireEvent.click(objectsTab);

    await waitFor(() => {
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('SPORE').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Fully on Bitcoin+CKB').length).toBeGreaterThanOrEqual(1);
      const links = screen.getAllByRole('link', { name: /Test Collection/i });
      expect(
        links.some(
          (link) =>
            link.getAttribute('href') ===
            '/clusters/0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890'
        )
      ).toBe(true);
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
      expect(screen.getByRole('button', { name: 'Sort by Used' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Used' }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'used', sortDirection: 'desc' })
      );
      const tokenLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/tokens/'));
      expect(tokenLinks[0]).toHaveTextContent('BETA');
    });
  });

  it('defaults to capacity sorting and toggles to ascending', async () => {
    vi.mocked(api.getAssets)
      .mockResolvedValueOnce(sortableTokenAssets)
      .mockResolvedValueOnce({
        ...sortableTokenAssets,
        data: [sortableTokenAssets.data[0], sortableTokenAssets.data[1]],
      });

    render(<AssetsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(
        expect.objectContaining({ sortKey: 'capacity', sortDirection: 'desc' })
      );
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

  it('maps object storage tier filters to API query', async () => {
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

  it('shows empty state when no assets found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByText('No assets found')).toBeInTheDocument();
    });
  });

  it('uses updated table headers and removes on-chain/transfers columns', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Standard' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Used' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Sort by Transfers' })).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /Objects/i }));

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Sort by On-chain' })).not.toBeInTheDocument();
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

  it('clears search and standard filters when switching tabs', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<AssetsPage />);

    const searchInput = screen.getByPlaceholderText('Search by name...');
    fireEvent.change(searchInput, { target: { value: 'test' } });

    const searchButton = screen.getByRole('button', { name: 'Search' });
    fireEvent.click(searchButton);

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
        expect.objectContaining({ type: 'object', search: undefined, standard: undefined })
      );
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
      expect(screen.getAllByText('Offchain Dependent').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('uses canonical slugs for identity collection links', async () => {
    const cases = [
      {
        assets: mockDotbitIdentityAssets,
        linkName: /\.bit/i,
        href: '/identities/dotbit',
        standard: 'DOTBIT',
      },
      {
        assets: mockDidCkbIdentityAssets,
        linkName: /did:ckb/i,
        href: '/identities/did:ckb',
        standard: 'did:ckb',
      },
    ];

    for (const testCase of cases) {
      vi.mocked(api.getAssets).mockResolvedValue(testCase.assets);
      mockReplace.mockClear();
      window.history.replaceState(null, '', '/assets');

      const view = render(<AssetsPage />);
      fireEvent.click(screen.getByRole('button', { name: /Identities/i }));

      await waitFor(() => {
        const links = screen.getAllByRole('link', { name: testCase.linkName });
        expect(links.some((link) => link.getAttribute('href') === testCase.href)).toBe(true);
        expect(screen.getAllByText(testCase.standard).length).toBeGreaterThan(0);
      });

      view.unmount();
    }
  });

  it('renders the HM column and supports sorting by HM', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    render(<AssetsPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by HM' })).toBeInTheDocument();
      expect(screen.getByText('×2.00')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sort by HM' }));
    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'hMultiplier', sortDirection: 'desc' })
      );
    });
  });
});
