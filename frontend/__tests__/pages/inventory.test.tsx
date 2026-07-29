import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TokensPage from '@/app/inventory/tokens/page';
import ObjectsPage from '@/app/inventory/objects/page';
import IdentitiesPage from '@/app/inventory/identities/page';
import { api } from '@/lib/api';

const mockReplace = vi.fn((href: string) => {
  const url = new URL(href, 'http://localhost');
  window.history.replaceState(null, '', `${url.pathname}${url.search}`);
});

vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(window.location.search),
  usePathname: () => window.location.pathname,
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
  isNetworkInitializingError: vi.fn(() => false),
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
      compositionTier: 'btc_ckb' as const,
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
      compositionTier: 'btc_ckb' as const,
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
      compositionTier: 'centralized_mixture' as const,
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

describe('Tokens Inventory Page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/inventory/tokens');
  });

  it('renders header, title, and token content', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<TokensPage />);

    await waitFor(() => {
      expect(screen.getByTestId('header')).toBeInTheDocument();
      expect(screen.getByText('Tokens')).toBeInTheDocument();
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'token' }));
      expect(screen.getAllByText('TEST').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('xUDT').length).toBeGreaterThan(0);
    });
  });

  it('marks total supply as raw when token decimals are unknown', async () => {
    // An unknown-decimals token must never render like a real 0-decimals
    // token: 1000000 base units is not "1,000,000 tokens".
    vi.mocked(api.getAssets).mockResolvedValue({
      ...mockTokenAssets,
      data: [{ ...mockTokenAssets.data[0], decimals: null, totalSupply: '1000000' }],
    });

    render(<TokensPage />);

    await waitFor(() => {
      expect(screen.getAllByText(/1,000,000 \(raw\)/).length).toBeGreaterThan(0);
    });
  });

  it('does not mark total supply for genuine 0-decimals tokens', async () => {
    vi.mocked(api.getAssets).mockResolvedValue({
      ...mockTokenAssets,
      data: [{ ...mockTokenAssets.data[0], decimals: 0, totalSupply: '1000000' }],
    });

    render(<TokensPage />);

    await waitFor(() => {
      expect(screen.getAllByText('1,000,000').length).toBeGreaterThan(0);
    });
    expect(screen.queryByText(/\(raw\)/)).toBeNull();
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

    render(<TokensPage />);

    const fallbackLabel = `${typeHash.slice(0, 10)}...${typeHash.slice(-8)}`;
    await waitFor(() => {
      expect(screen.getAllByText(fallbackLabel).length).toBeGreaterThanOrEqual(1);
    });
  });

  it('shows empty state when no tokens found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<TokensPage />);

    await waitFor(() => {
      expect(screen.getByText('No tokens found')).toBeInTheDocument();
    });
  });

  it('renders sort headers and supports sorting', async () => {
    vi.mocked(api.getAssets)
      .mockResolvedValueOnce(sortableTokenAssets)
      .mockResolvedValueOnce({
        ...sortableTokenAssets,
        data: [sortableTokenAssets.data[1], sortableTokenAssets.data[0]],
      });

    render(<TokensPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by Standard' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Used' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sort by Capacity' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Sort by Used' }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'used', sortDirection: 'desc' })
      );
    });
  });

  it('defaults to capacity sorting and toggles to ascending', async () => {
    vi.mocked(api.getAssets)
      .mockResolvedValueOnce(sortableTokenAssets)
      .mockResolvedValueOnce({
        ...sortableTokenAssets,
        data: [sortableTokenAssets.data[0], sortableTokenAssets.data[1]],
      });

    render(<TokensPage />);

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
    });
  });

  it('handles search functionality', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<TokensPage />);

    const searchInput = screen.getByPlaceholderText('Search by name...');
    fireEvent.change(searchInput, { target: { value: 'test' } });

    const searchButton = screen.getByRole('button', { name: 'Search' });
    fireEvent.click(searchButton);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ search: 'test' }));
    });
  });

  it('filters by selected standard', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<TokensPage />);

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

  it('renders the HMul column and supports sorting by HMul', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    render(<TokensPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Sort by HMul' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Explain HMul' })).toBeInTheDocument();
      expect(screen.getByText('\u00d72.00')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sort by HMul' }));
    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'hMultiplier', sortDirection: 'desc' })
      );
    });
  });
});

describe('Objects Inventory Page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/inventory/objects');
  });

  it('renders object collection content and cluster link', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('Objects')).toBeInTheDocument();
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText('Composition')).toBeInTheDocument();
      expect(screen.getAllByText('SPORE').length).toBeGreaterThan(0);
      const tierLabels = screen.getAllByText('BTC+CKB');
      expect(tierLabels.length).toBeGreaterThanOrEqual(1);
      const links = screen.getAllByRole('link', { name: /Test Collection/i });
      expect(
        links.some(
          (link) =>
            link.getAttribute('href') ===
            '/mainnet/clusters/0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890'
        )
      ).toBe(true);
    });
  });

  it('maps composition tier filters to API query', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', compositionTier: undefined })
      );
    });

    fireEvent.change(screen.getByLabelText('Filter by composition tier'), {
      target: { value: 'pure_ckb' },
    });

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', compositionTier: 'pure_ckb' })
      );
      expect(window.location.search).toContain('compositionTier=pure_ckb');
    });
  });

  it('shows empty state when no objects found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      expect(screen.getByText('No objects found')).toBeInTheDocument();
    });
  });

  it('renders mixed object types with composition tiers', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockMixedObjectAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      expect(screen.getAllByText('MNFT Without Icon').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('Centralized Mixture').length).toBeGreaterThanOrEqual(1);
    });
  });
});

describe('Identities Inventory Page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/inventory/identities');
  });

  it('shows did:ckb in Identity standards', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockDidCkbIdentityAssets);

    render(<IdentitiesPage />);

    await waitFor(() => {
      expect(screen.getByText('Identities')).toBeInTheDocument();
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'identity' }));
    });

    expect(screen.getByRole('option', { name: 'did:ckb' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'D-ID' })).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText('Filter by standard'), {
      target: { value: 'did:ckb' },
    });
    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'identity', standard: 'did:ckb' })
      );
      expect(window.location.search).toContain('standard=did%3Ackb');
    });
  });

  it('shows empty state when no identities found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<IdentitiesPage />);

    await waitFor(() => {
      expect(screen.getByText('No identities found')).toBeInTheDocument();
    });
  });

  it('uses canonical slugs for identity collection links', async () => {
    const cases = [
      {
        assets: mockDotbitIdentityAssets,
        linkName: /\.bit/i,
        href: '/mainnet/identities/dotbit',
        standard: 'DOTBIT',
      },
      {
        assets: mockDidCkbIdentityAssets,
        linkName: /did:ckb/i,
        href: '/mainnet/identities/did:ckb',
        standard: 'did:ckb',
      },
    ];

    for (const testCase of cases) {
      vi.mocked(api.getAssets).mockResolvedValue(testCase.assets);
      mockReplace.mockClear();
      window.history.replaceState(null, '', '/inventory/identities');

      const view = render(<IdentitiesPage />);

      await waitFor(() => {
        const links = screen.getAllByRole('link', { name: testCase.linkName });
        expect(links.some((link) => link.getAttribute('href') === testCase.href)).toBe(true);
        expect(screen.getAllByText(testCase.standard).length).toBeGreaterThan(0);
      });

      view.unmount();
    }
  });
});
