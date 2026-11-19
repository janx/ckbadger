import { CHARTS } from '@/components/charts/chart-catalogue';
import { describe, it, expect, vi, beforeEach, onTestFinished } from 'vitest';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import { render, createTestQueryClient } from '@/__tests__/utils/test-utils';
import { QueryClientProvider } from '@tanstack/react-query';
import TokensPage from '@/app/inventory/tokens/page';
import ObjectsPage from '@/app/inventory/objects/page';
import IdentitiesPage from '@/app/inventory/identities/page';
import { api } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';

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
    getInventorySummary: vi.fn(),
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
      liveCount: null,
      activitiesCount: null,
      compositionTier: null,
      onchainRatio: null,
      onchainCount: null,
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
      hMultiplier: '2.00',
    },
  ],
  total: 1,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

/** One token a page, two pages: the first with an older one after it, then the last. */
const twoTokenPages = (cursor: string | undefined) =>
  cursor
    ? pagedRead({ ...mockTokenAssets, limit: 1, total: 2, hasMore: false, nextCursor: null }, 1)
    : pagedRead({ ...mockTokenAssets, limit: 1, total: 2, hasMore: true, nextCursor: 'older' });

const mockClusterAssets = {
  data: [
    {
      id: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
      assetType: 'object' as const,
      liveCount: null,
      activitiesCount: null,
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
      onchainRatio: '1.0000',
      onchainCount: 50,
      hMultiplier: '1.80',
    },
  ],
  total: 1,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

const mockDotbitIdentityAssets = {
  data: [
    {
      id: '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f',
      assetType: 'identity' as const,
      liveCount: null,
      activitiesCount: null,
      compositionTier: null,
      onchainRatio: null,
      onchainCount: null,
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
      hMultiplier: '2.33',
    },
  ],
  total: 1,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

const mockDidCkbIdentityAssets = {
  data: [
    {
      id: '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f',
      assetType: 'identity' as const,
      liveCount: null,
      activitiesCount: null,
      compositionTier: null,
      onchainRatio: null,
      onchainCount: null,
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
      hMultiplier: '1.60',
    },
  ],
  total: 1,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

const mockMixedObjectAssets = {
  data: [
    {
      id: '0x1111111111111111111111111111111111111111111111111111111111111111',
      assetType: 'object' as const,
      liveCount: null,
      activitiesCount: null,
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
      onchainRatio: '1.0000',
      onchainCount: 10,
      hMultiplier: '3.00',
    },
    {
      id: '0x2222222222222222222222222222222222222222222222222222222222222222',
      assetType: 'object' as const,
      liveCount: null,
      activitiesCount: null,
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
      onchainRatio: '0.0000',
      onchainCount: 0,
      hMultiplier: '2.22',
    },
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

const emptyAssets = {
  data: [],
  total: 0,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

const sortableTokenAssets = {
  data: [
    {
      id: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      assetType: 'token' as const,
      liveCount: null,
      activitiesCount: null,
      compositionTier: null,
      onchainRatio: null,
      onchainCount: null,
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
      hMultiplier: '2.00',
    },
    {
      id: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      assetType: 'token' as const,
      liveCount: null,
      activitiesCount: null,
      compositionTier: null,
      onchainRatio: null,
      onchainCount: null,
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
      hMultiplier: '1.80',
    },
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
};

describe('Tokens Inventory Page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getInventorySummary).mockResolvedValue({
      source: 'canonical_asset_indexes',
      anchor: null,
      counts: { token: 500, object: 200, identity: 4 },
      objectStandards: [],
      objectComposition: [
        { tier: 'pure_ckb', collectionsCount: 20, liveCount: 100 },
        { tier: 'centralized_mixture', collectionsCount: 180, liveCount: 900 },
      ],
      fullyOnchainObjectsCount: 1000,
      tokenStandards: [
        { standard: 'sudt', count: 100 },
        { standard: 'xudt', count: 398 },
        { standard: 'xudt_compatible', count: 2 },
      ],
      pendingItemsCount: 3,
    });
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/inventory/tokens');
  });

  // L4 (responsive plan, board L3): below sm the standard chips fold into a select; a phone reads
  // twenty. Re-pinned on purpose (controls lane C2): the fold is the filter chips' own phone form
  // (the four standards are 397.6 px of chips, wider than a 360 px window's 328 px line), a select
  // after the filter's label "standard" in the filter's row beside the search, and the sort is the
  // select after "sorted by", shown at every width. It was the page's own select on the sort line
  // (sm:hidden, "Standard filter" for a screen reader, "All 500" / "xUDT 398" …), the chips
  // hidden below sm, and "sorted by" for a screen reader only below sm.
  it('folds the standard chips into a select after "standard" below sm and reads twenty rows on a phone', async () => {
    stubViewportFromMd(false);
    onTestFinished(() => {
      vi.unstubAllGlobals();
    });
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    render(<TokensPage />);
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ limit: 20 }))
    );
    expect(api.getAssets).not.toHaveBeenCalledWith(expect.objectContaining({ limit: 50 }));
    const standard = await screen.findByRole('combobox', { name: 'standard' });
    await waitFor(() =>
      expect(
        within(standard)
          .getAllByRole('option')
          .map((o) => o.textContent)
      ).toEqual(['all 500', 'xudt 398', 'sudt 100', 'xudt-compatible 2'])
    );
    expect(screen.queryByRole('radiogroup', { name: 'standard' })).toBeNull();
    const controls = screen.getByTestId('inventory-controls');
    expect(within(controls).getByText('sorted by')).not.toHaveClass('sr-only');
    expect(within(controls).getByRole('combobox', { name: 'sorted by' })).toBeInTheDocument();
    fireEvent.change(standard, { target: { value: 'sudt' } });
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ standard: 'sudt', limit: 20 })
      )
    );
  });

  // Lane L (charts ↔ topics): every inventory view links the charts drawn from the inventory.
  it('links every chart drawn from the inventory (lane L)', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    render(<TokensPage />);
    const charts = await screen.findByRole('navigation', { name: 'Charts of these numbers' });
    const hrefs = within(charts)
      .getAllByRole('link')
      .map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(
      CHARTS.filter((c) => c.evidenceHref === '/inventory').map((c) => `/mainnet/charts/${c.slug}`)
    );
  });

  it('renders the title and token content', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<TokensPage />);

    await waitFor(() => {
      // Re-pinned on purpose (controls lane C2): the class is the current tab of the command
      // line, its name lowercase (it was the nav link "Tokens"). Re-pinned again (lane c2-finish,
      // item 3): each class is its own address, so it is the tab row's link, the current page.
      expect(screen.getByRole('link', { name: /^tokens/ })).toHaveAttribute('aria-current', 'page');
      expect(api.getAssets).toHaveBeenCalledWith(expect.objectContaining({ type: 'token' }));
      // F2 objects (IV-04): the full name leads. Table-rows L2-1: the symbol is a tag beside it
      // and the standard is in the row's title.
      expect(screen.getAllByText('Test Token').length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText('TEST')).toHaveAttribute('data-row-tag', 'default');
      expect(screen.getByRole('table', { name: 'Tokens' })).toContainHTML(' · TEST · xUDT · ');
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

  it('scales total supply exactly for decimals beyond double precision', async () => {
    // decimals is the raw, unvalidated first byte of the xUDT Unique Cell, so
    // >= 23 is reachable. A BigInt(10 ** 24) divisor rendered 10^24 base units
    // as "1.000000000000000016777216".
    vi.mocked(api.getAssets).mockResolvedValue({
      ...mockTokenAssets,
      data: [
        { ...mockTokenAssets.data[0], decimals: 24, totalSupply: '1000000000000000000000000' },
      ],
    });

    render(<TokensPage />);

    // The supply is the list's summary form: exactly 1, so it is not cut and carries no title
    // (readability R5, re-pinned on purpose: TokenTwoPlaces titled every amount, "1"); a float
    // would have made it
    // 1.000000000000000016777216, cut to "1" and titled with that.
    await waitFor(() => {
      expect(
        [...document.querySelectorAll('[data-token-amount]')].some(
          (el) => el.textContent === '1' && !el.hasAttribute('title')
        )
      ).toBe(true);
    });
    expect(screen.queryByText(/1\.000000000000000016777216/)).toBeNull();
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

    // U-09 (re-pinned): the table's name cell at the row size
    const fallbackLabel = `${typeHash.slice(0, 10)}…${typeHash.slice(-6)}`;
    await waitFor(() => {
      expect(screen.getAllByText(fallbackLabel).length).toBeGreaterThanOrEqual(1);
    });
  });

  it('shows empty state when no tokens found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<TokensPage />);

    await waitFor(() => {
      expect(screen.getByText('No tokens.')).toBeInTheDocument();
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

    // Re-pinned on purpose (controls lane C2): the controls board's select, named by the label
    // it stands after, "sorted by" (it was the aria-label "Sort inventory").
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'sorted by' })).toHaveValue('capacity');
    });

    fireEvent.change(screen.getByRole('combobox', { name: 'sorted by' }), {
      target: { value: 'used' },
    });

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
      expect(screen.getByRole('button', { name: 'Reverse sort direction' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reverse sort direction' }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'capacity', sortDirection: 'asc' })
      );
    });
  });

  it('handles search functionality', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);

    render(<TokensPage />);

    // F2 objects (IV-07): the board's search box filters as you type; there is no button. Its
    // placeholder is lowercase without dots, as the header's (U-01, R2).
    const searchInput = screen.getByPlaceholderText('search by name');
    fireEvent.change(searchInput, { target: { value: 'test' } });

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

    // Re-pinned on purpose (controls lane C2): the controls board's chip, a radio named by its
    // lowercase words (it was the aria-pressed button "xUDT 398").
    fireEvent.click(await screen.findByRole('radio', { name: 'xudt 398' }));

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'token', standard: 'xudt' })
      );
      expect(window.location.search).toBe('?standard=xudt');
    });
  });

  // The controls lane (C2, c2-lists): the filter holds one value of its own, so a standard the URL
  // chose that the summary does not list (no catalog token, by the summary's own count) stands as
  // the chosen chip with none (0), and the list asks for it as before; another choice drops it.
  it('names a URL standard the summary does not list as the chosen chip, with none', async () => {
    window.history.replaceState(null, '', '/inventory/tokens?standard=spore');
    vi.mocked(api.getAssets).mockResolvedValue({ ...mockTokenAssets, data: [], total: 0 });
    render(<TokensPage />);
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'token', standard: 'spore' })
      )
    );
    const chips = await screen.findByRole('radiogroup', { name: 'standard' });
    expect(
      within(chips)
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual(['all 500', 'xudt 398', 'sudt 100', 'xudt-compatible 2', 'spore 0']);
    expect(within(chips).getByRole('radio', { name: 'spore 0' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(within(chips).getByRole('radio', { name: 'spore 0' })).toHaveClass('text-text-bright');
    fireEvent.click(within(chips).getByRole('radio', { name: 'all 500' }));
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'token', standard: undefined })
      )
    );
    expect(window.location.search).toBe('');
    expect(within(chips).queryByRole('radio', { name: 'spore 0' })).toBeNull();
  });

  it('renders the HMul column and supports sorting by HMul', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(sortableTokenAssets);
    render(<TokensPage />);
    // Re-pinned on purpose (controls lane C2): the select's choices in the controls board's
    // lowercase ("h-mult.", was "H-mult."), the select named by "sorted by".
    await waitFor(() => {
      expect(screen.getByRole('option', { name: 'h-mult.' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Explain HMul' })).toBeInTheDocument();
      expect(screen.getAllByText('\u00d72.00').length).toBeGreaterThan(0);
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'sorted by' }), {
      target: { value: 'hMultiplier' },
    });
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
    vi.mocked(api.getInventorySummary).mockResolvedValue({
      source: 'canonical_asset_indexes',
      anchor: null,
      counts: { token: 500, object: 200, identity: 4 },
      objectStandards: [],
      objectComposition: [
        { tier: 'pure_ckb', collectionsCount: 20, liveCount: 100 },
        { tier: 'centralized_mixture', collectionsCount: 180, liveCount: 900 },
      ],
      fullyOnchainObjectsCount: 1000,
      tokenStandards: [
        { standard: 'sudt', count: 100 },
        { standard: 'xudt', count: 398 },
        { standard: 'xudt_compatible', count: 2 },
      ],
      pendingItemsCount: 3,
    });
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/inventory/objects');
  });

  it('renders object collection content and cluster link', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      // Re-pinned on purpose (controls lane C2): the current tab of the command line (it was the
      // nav link "Objects"). Re-pinned again (lane c2-finish, item 3): the tab row's link, the
      // current page (a class is its own address).
      expect(screen.getByRole('link', { name: /^objects/ })).toHaveAttribute(
        'aria-current',
        'page'
      );
      expect(screen.getAllByText('Test Collection').length).toBeGreaterThanOrEqual(1);
      // Table-rows L2-2: the composition is the row's second tag, not a column of its own.
      expect(screen.queryByRole('columnheader', { name: 'Composition' })).toBeNull();
      // F2 objects: no standard chips on Objects (board); the row names its standard (a tag).
      expect(screen.getAllByText('Spore')[0]).toHaveAttribute('data-row-tag', 'default');
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

  it('keeps decode pending visible and neutral in collection table and cards', async () => {
    vi.mocked(api.getAssets).mockResolvedValue({
      ...mockClusterAssets,
      data: [
        {
          ...mockClusterAssets.data[0],
          compositionTier: 'decode_pending',
          onchainRatio: null,
          onchainCount: null,
        },
      ],
    });
    const { unmount } = render(<ObjectsPage />);
    await screen.findByRole('table', { name: 'Object collections' });
    const neutral = () => {
      const labels = screen.getAllByText('Decode pending').filter((el) => el.tagName === 'SPAN');
      expect(labels).toHaveLength(1);
      for (const label of labels) {
        // re-pinned on purpose (readability P2, lane p-assets): the composition chip's words read
        // in the reading ink, as the composition legend writes them (they were the quiet ink); a
        // row tag keeps the quiet ink of a label; neither warns
        expect(label).toHaveClass(
          label.hasAttribute('data-composition') ? 'text-text' : 'text-text-dim'
        );
        expect(label).not.toHaveClass('text-warning', 'storage-text-gem');
      }
    };
    neutral();
    unmount();
    // The phone row (board L4) keeps the same neutral mark.
    atContainerWidth(358);
    render(<ObjectsPage />);
    await screen.findByTestId('inventory-row-mobile');
    neutral();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('maps composition tier filters to API query', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockClusterAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ type: 'object', compositionTier: undefined })
      );
    });

    // F2 objects (IV-09): composition chips with their counts are the filter.
    fireEvent.click(await screen.findByRole('button', { name: /Pure CKB 20/ }));

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
      expect(screen.getByText('No objects.')).toBeInTheDocument();
    });
  });

  it('renders mixed object types with composition tiers', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockMixedObjectAssets);

    render(<ObjectsPage />);

    await waitFor(() => {
      expect(screen.getAllByText('MNFT Without Icon').length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText('Centralized mix').length).toBeGreaterThanOrEqual(1);
    });
  });
});

describe('Identities Inventory Page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getInventorySummary).mockResolvedValue({
      source: 'canonical_asset_indexes',
      anchor: null,
      counts: { token: 500, object: 200, identity: 4 },
      objectStandards: [],
      objectComposition: [
        { tier: 'pure_ckb', collectionsCount: 20, liveCount: 100 },
        { tier: 'centralized_mixture', collectionsCount: 180, liveCount: 900 },
      ],
      fullyOnchainObjectsCount: 1000,
      tokenStandards: [
        { standard: 'sudt', count: 100 },
        { standard: 'xudt', count: 398 },
        { standard: 'xudt_compatible', count: 2 },
      ],
      pendingItemsCount: 3,
    });
    mockReplace.mockClear();
    window.history.replaceState(null, '', '/inventory/identities');
  });

  /// The four identity standards the indexer writes, filtered by the wire
  /// value each one has in `IdentityStandard::as_str()`. A display label may
  /// read however it likes; the value that travels to the API may not.
  // F2 objects (IV-12): the Identities board lists the four systems without standard chips; a
  // standard in the URL still travels to the API as its wire value.
  it('offers no standard chips and filters a URL standard by its wire value', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockDidCkbIdentityAssets);
    for (const wire of ['dotbit', 'bit_cell', 'did_ckb', 'dotcell']) {
      window.history.replaceState(null, '', `/inventory/identities?standard=${wire}`);
      const view = render(<IdentitiesPage />);
      await waitFor(() =>
        expect(api.getAssets).toHaveBeenLastCalledWith(
          expect.objectContaining({ type: 'identity', standard: wire })
        )
      );
      // Re-pinned on purpose (controls lane C2): the filter is the controls board's chips (a
      // radio group) or their phone select, both named "standard" (it was "Standard filters").
      expect(screen.queryByRole('radiogroup', { name: 'standard' })).not.toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: 'standard' })).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it('shows empty state when no identities found', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(emptyAssets);

    render(<IdentitiesPage />);

    await waitFor(() => {
      expect(screen.getByText('No identities.')).toBeInTheDocument();
    });
  });

  it('uses canonical slugs for identity collection links', async () => {
    const cases = [
      {
        assets: mockDotbitIdentityAssets,
        linkName: /\.bit/i,
        href: '/mainnet/identities/dotbit',
        standard: '.bit',
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

describe('Inventory approved board regressions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getInventorySummary).mockResolvedValue({
      source: 'canonical_asset_indexes',
      anchor: null,
      counts: { token: 500, object: 200, identity: 4 },
      objectStandards: [],
      objectComposition: [
        { tier: 'pure_ckb', collectionsCount: 20, liveCount: 100 },
        { tier: 'centralized_mixture', collectionsCount: 180, liveCount: 900 },
      ],
      fullyOnchainObjectsCount: 1000,
      tokenStandards: [
        { standard: 'sudt', count: 100 },
        { standard: 'xudt', count: 398 },
        { standard: 'xudt_compatible', count: 2 },
      ],
      pendingItemsCount: 3,
    });
    window.history.replaceState(null, '', '/inventory/objects');
  });
  it('distinguishes live objects, minted objects, and actual activities', async () => {
    vi.mocked(api.getAssets).mockResolvedValue({
      ...mockClusterAssets,
      data: [{ ...mockClusterAssets.data[0], liveCount: 41, activitiesCount: 123 }],
    });
    render(<ObjectsPage />);
    const table = await screen.findByRole('table', { name: 'Object collections' });
    expect(within(table).getByText('41')).toBeInTheDocument();
    // Table-rows L2-2: minted ≠ live is a dim "/ 50" (R4, re-pinned on purpose); the words are in
    // the row's title.
    expect(within(table).getByText('/ 50', { exact: false })).toHaveClass('text-text-dim');
    expect(within(table).getAllByRole('row')[1].getAttribute('title')).toContain(
      '41 live of 50 minted'
    );
    expect(within(table).getByText('123')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Inventory', level: 1 })).toBeInTheDocument();
    // Re-pinned on purpose (controls lane C2): the classes are the command line's tablist, their
    // names lowercase (it was the nav "Inventory classes" with "Tokens"). Re-pinned again (lane
    // c2-finish, item 3): the tab row's links, a navigation again, its names lowercase still.
    expect(screen.getByRole('navigation', { name: 'Inventory classes' })).toHaveTextContent(
      'tokens'
    );
  });
  it('shows transport errors as errors, not an empty inventory', async () => {
    vi.mocked(api.getAssets).mockRejectedValue(new Error('pinned read failed'));
    render(<ObjectsPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('pinned read failed');
    expect(screen.queryByText('No objects.')).not.toBeInTheDocument();
  });
});

describe('Inventory snapshot and pagination lifecycle', () => {
  it('shows canonical global counts separately from a cached page', async () => {
    vi.mocked(api.getInventorySummary).mockResolvedValue({
      source: 'canonical_asset_indexes',
      anchor: { blockNumber: 99, blockHash: '0xaa' },
      counts: { token: 500, object: 200, identity: 4 },
      objectStandards: [],
      objectComposition: [
        { tier: 'pure_ckb', collectionsCount: 20, liveCount: 100 },
        { tier: 'centralized_mixture', collectionsCount: 180, liveCount: 900 },
      ],
      fullyOnchainObjectsCount: 1000,
      tokenStandards: [
        { standard: 'sudt', count: 100 },
        { standard: 'xudt', count: 398 },
        { standard: 'xudt_compatible', count: 2 },
      ],
      pendingItemsCount: 3,
    });
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    render(<TokensPage />);
    // Re-pinned on purpose (controls lane C2): the command line's tablist, lowercase names (it was
    // the nav "Inventory classes" with "Tokens"). Re-pinned again (lane c2-finish, item 3): the tab
    // row's links, a navigation again (each class is its own address).
    expect(await screen.findByRole('navigation', { name: 'Inventory classes' })).toHaveTextContent(
      'tokens'
    );
    await waitFor(() =>
      expect(screen.getByRole('navigation', { name: 'Inventory classes' })).toHaveTextContent('500')
    );
    expect(screen.getByLabelText('Inventory totals')).toHaveTextContent(
      'cached snapshot and can lag'
    );
  });
  // Re-pinned on purpose (controls lane C2): the controls board's pager and chips — "next →",
  // the radio "sudt 100" (they were the buttons "Next" and "sUDT 100"); the fixture's total says
  // the second page it serves (52 rows, it said 2 beside a next page, which the old pager drew and
  // the new one refuses).
  it('resets the cursor when a standard chip changes', async () => {
    // re-pinned (lane cd-fixes-controls, item 1): two pages that agree with themselves
    // (pagedRead), the first with an older one after it, then that older and last one (it served
    // the same page at every cursor, whose next cursor repeated the one it was asked with: an API
    // fault the pager now refuses; and one token on a page of 50 with more after it)
    vi.mocked(api.getAssets).mockImplementation(async (p) => twoTokenPages(p?.cursor));
    render(<TokensPage />);
    await screen.findAllByText('Test Token');
    fireEvent.click(screen.getByRole('button', { name: 'next →' }));
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: 'older' }))
    );
    fireEvent.click(await screen.findByRole('radio', { name: 'sudt 100' }));
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ standard: 'sudt', cursor: undefined })
      )
    );
  });
  // The controls lane (C2, c2-lists), written before the sort and the search became the controls
  // board's select and field: each starts the list again from its first page.
  it('starts the list again from its first page when the sort changes', async () => {
    // re-pinned (lane cd-fixes-controls, item 1): two pages that agree with themselves
    // (pagedRead), the first with an older one after it, then that older and last one (it served
    // the same page at every cursor, whose next cursor repeated the one it was asked with: an API
    // fault the pager now refuses; and one token on a page of 50 with more after it)
    vi.mocked(api.getAssets).mockImplementation(async (p) => twoTokenPages(p?.cursor));
    render(<TokensPage />);
    await screen.findAllByText('Test Token');
    fireEvent.click(screen.getByRole('button', { name: /^next/i }));
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: 'older' }))
    );
    fireEvent.change(screen.getByRole('combobox', { name: /sort/i }), {
      target: { value: 'name' },
    });
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'name', cursor: undefined })
      )
    );
  });
  it('starts the list again from its first page when the search changes', async () => {
    // re-pinned (lane cd-fixes-controls, item 1): two pages that agree with themselves
    // (pagedRead), the first with an older one after it, then that older and last one (it served
    // the same page at every cursor, whose next cursor repeated the one it was asked with: an API
    // fault the pager now refuses; and one token on a page of 50 with more after it)
    vi.mocked(api.getAssets).mockImplementation(async (p) => twoTokenPages(p?.cursor));
    render(<TokensPage />);
    await screen.findAllByText('Test Token');
    fireEvent.click(screen.getByRole('button', { name: /^next/i }));
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: 'older' }))
    );
    fireEvent.change(screen.getByPlaceholderText('search by name'), { target: { value: 'beta' } });
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: 'beta', cursor: undefined })
      )
    );
  });
  it('retains the loaded rows and makes refetch failures visible', async () => {
    vi.mocked(api.getAssets).mockResolvedValue(mockTokenAssets);
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>
    );
    await screen.findAllByText('Test Token');
    vi.mocked(api.getAssets).mockRejectedValue(new Error('Cache read failed'));
    await client.invalidateQueries({ queryKey: ['assets'] });
    expect(await screen.findByRole('alert')).toHaveTextContent('Showing previously loaded');
    expect(screen.getAllByText('Test Token').length).toBeGreaterThan(0);
  });
});
