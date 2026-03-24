import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { DEFAULT_API_BASE } from '@/lib/runtime-config';
import type { Cell } from '@/lib/api';
import { InventoryContextSection } from '@/components/cell/inventory-context';

const API_BASE = DEFAULT_API_BASE;

function renderWithProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>
  );
}

function makeCell(overrides: Partial<Cell> = {}): Cell {
  return {
    txHash: '0xabc123def456789012345678901234567890123456789012345678901234abcd',
    outputIndex: 0,
    capacity: '14500000000',
    dataSize: 100,
    createdAtBlock: 100000,
    lockScriptHash: '0xlockhash',
    status: 'live' as const,
    isDepGroup: false,
    ...overrides,
  };
}

describe('InventoryContextSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders nothing when cell has no type script', () => {
    const cell = makeCell({ type: undefined });
    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders nothing for unrecognized deterministic kind', () => {
    const cell = makeCell({
      type: { codeHash: '0xunknown', hashType: 'type', args: '0xargs' },
      dataAnalysis: {
        deterministic: {
          kind: 'some_unknown_kind',
          summary: 'Unknown',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });
    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders Spore card with content type and cluster link', async () => {
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          summary: 'Spore NFT',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('Spore NFT')).toBeInTheDocument();
    });

    expect(screen.getByText('image/png')).toBeInTheDocument();

    const clusterLink = screen.getByRole('link', { name: /0xcluster456/i });
    expect(clusterLink).toHaveAttribute('href', '/clusters/0xcluster456');
  });

  it('renders UDT card with formatted amount and symbol', async () => {
    const cell = makeCell({
      type: { codeHash: '0xudt_code', hashType: 'type', args: '0xargs' },
      typeScriptHash: '0xtokenhash123',
      udtAmount: '12345678900000000',
      dataAnalysis: {
        deterministic: {
          kind: 'udt_amount',
          summary: 'UDT amount',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('Test Token')).toBeInTheDocument();
    });

    expect(screen.getByText('123,456,789')).toBeInTheDocument();
    expect(screen.getByText('TT')).toBeInTheDocument();
  });

  it('renders m-NFT token card with class and issuer names', async () => {
    const cell = makeCell({
      type: { codeHash: '0xmnft_code', hashType: 'type', args: '0xmnft_token_id' },
      dataAnalysis: {
        deterministic: {
          kind: 'mnft_token_cell',
          summary: 'M-NFT Token',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('Test NFT Class')).toBeInTheDocument();
    });

    expect(screen.getByText('Test Issuer')).toBeInTheDocument();
  });

  it('renders Cluster card with name and item count', async () => {
    const cell = makeCell({
      type: { codeHash: '0xcluster_code', hashType: 'type', args: '0xcluster456' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cluster_cell',
          summary: 'Spore Cluster',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('Test Cluster')).toBeInTheDocument();
    });

    expect(screen.getByText('42')).toBeInTheDocument();
  });

  it('renders .bit card with account name', async () => {
    const cell = makeCell({
      type: { codeHash: '0xdotbit_code', hashType: 'type', args: '0xdotbit_account_id' },
      dataAnalysis: {
        deterministic: {
          kind: 'dotbit_account',
          summary: '.bit Account',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('alice.bit')).toBeInTheDocument();
    });
  });

  it('shows loading skeleton while fetching', async () => {
    // Add a delay to the spore endpoint so we can catch the loading state
    server.use(
      http.get(`${API_BASE}/spore/objects/:sporeId`, async () => {
        await new Promise((resolve) => setTimeout(resolve, 500));
        return HttpResponse.json({
          sporeId: '0xspore123',
          txHash: '0xabc123',
          outputIndex: 0,
          clusterId: '0xcluster456',
          contentType: 'image/png',
          contentSize: 1024,
          ownerLockHash: '0xownerhash789',
          isLive: true,
          createdAtBlock: 100000,
          ownedCapacity: '14500000000',
          ownedKnowledge: null,
          mediaProfile: null,
        });
      })
    );

    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          summary: 'Spore NFT',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    expect(screen.getByTestId('inventory-context-loading')).toBeInTheDocument();
  });

  it('hides section silently on API error', async () => {
    server.use(
      http.get(`${API_BASE}/spore/objects/:sporeId`, () => {
        return new HttpResponse(null, { status: 404 });
      })
    );

    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          summary: 'Spore NFT',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);

    // Wait for the query to settle (error state)
    await waitFor(() => {
      expect(screen.queryByTestId('inventory-context-loading')).not.toBeInTheDocument();
    });

    // Should render nothing on error
    expect(container.querySelector('[data-testid="inventory-context-section"]')).toBeNull();
  });

  it('renders DID:CKB card via code_hash fallback when no deterministic kind', async () => {
    const cell = makeCell({
      type: {
        codeHash: '0x079bb8c1dfb249f60d932f4b1a60fa5cb2a36af3653ac09464f262e2f3f682a9',
        hashType: 'type',
        args: '0xdid_ckb_id',
      },
      // No dataAnalysis.deterministic — triggers fallback detection
      dataAnalysis: undefined,
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('DID:CKB Identity')).toBeInTheDocument();
    });
  });

  it('renders nothing for DAO deposit cell kind', () => {
    const cell = makeCell({
      type: {
        codeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
        hashType: 'type',
        args: '0x',
      },
      dataAnalysis: {
        deterministic: {
          kind: 'dao_deposit_cell',
          summary: 'DAO Deposit',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    const { container } = renderWithProviders(<InventoryContextSection cell={cell} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders correct view details link for spore type', async () => {
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          summary: 'Spore NFT',
          segments: [],
        },
        heuristicGuesses: [],
      },
    });

    renderWithProviders(<InventoryContextSection cell={cell} />);

    await waitFor(() => {
      expect(screen.getByText('Spore NFT')).toBeInTheDocument();
    });

    const viewLink = screen.getByRole('link', { name: /View details/i });
    expect(viewLink).toHaveAttribute('href', '/objects/0xspore123');
  });
});
