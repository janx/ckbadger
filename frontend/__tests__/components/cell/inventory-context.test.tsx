import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { DEFAULT_API_BASE } from '@/lib/runtime-config';
import type { Cell } from '@/lib/api';
import { useInventoryLabel } from '@/components/cell/inventory-context';

const API_BASE = DEFAULT_API_BASE;

function wrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
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

describe('useInventoryLabel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('returns null when cell has no type script', () => {
    const cell = makeCell({ type: undefined });
    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });
    expect(result.current).toBeNull();
  });

  it('returns null when cell is undefined', () => {
    const { result } = renderHook(() => useInventoryLabel(undefined), { wrapper });
    expect(result.current).toBeNull();
  });

  it('returns null for unrecognized deterministic kind', () => {
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
    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });
    expect(result.current).toBeNull();
  });

  it('returns null for DAO deposit cell kind', () => {
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
    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });
    expect(result.current).toBeNull();
  });

  it('returns Spore Object label with content info from segments', () => {
    const cell = makeCell({
      type: { codeHash: '0xspore_code', hashType: 'type', args: '0xspore123' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          summary: 'Spore Object',
          segments: [
            {
              label: 'content_type',
              start: 0,
              end: 9,
              meaning: 'Content MIME type',
              humanValue: 'image/png',
            },
            {
              label: 'content',
              start: 9,
              end: 1033,
              meaning: 'Content data',
              humanValue: '1024 bytes',
            },
          ],
        },
        heuristicGuesses: [],
      },
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('Spore Object');
    expect(result.current!.summary).toContain('image/png');
    expect(result.current!.href).toBe('/objects/0xspore123');
  });

  it('returns Cluster label with name from segments', () => {
    const cell = makeCell({
      type: { codeHash: '0xcluster_code', hashType: 'type', args: '0xcluster456' },
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cluster_cell',
          summary: 'Spore Cluster',
          segments: [
            {
              label: 'name',
              start: 0,
              end: 12,
              meaning: 'Cluster name',
              humanValue: 'Test Cluster',
            },
          ],
        },
        heuristicGuesses: [],
      },
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('Spore Cluster');
    expect(result.current!.displayName).toBe('Test Cluster');
    expect(result.current!.href).toBe('/clusters/0xcluster456');
  });

  it('returns UDT label with amount and symbol after token fetch', async () => {
    server.use(
      http.get(`${API_BASE}/tokens/:tokenId`, () => {
        return HttpResponse.json({
          typeScriptHash: '0xtokenhash123',
          name: 'Test Token',
          symbol: 'TT',
          decimals: 8,
          totalSupply: '100000000000000000',
          holdersCount: 100,
          circulatingSupply: '50000000000000000',
        });
      })
    );

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

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    // Initially returns label without summary (token not yet fetched)
    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('Token (UDT)');
    expect(result.current!.href).toBe('/tokens/0xtokenhash123');

    // After token data loads, displayName and summary should be populated
    await waitFor(() => {
      expect(result.current!.displayName).not.toBeNull();
    });

    expect(result.current!.displayName).toBe('TT');
    expect(result.current!.summary).toContain('TT');
  });

  it('returns .bit label with account name from segments', () => {
    const cell = makeCell({
      type: { codeHash: '0xdotbit_code', hashType: 'type', args: '0xdotbit_account_id' },
      dataAnalysis: {
        deterministic: {
          kind: 'dotbit_account',
          summary: '.bit Account',
          segments: [
            {
              label: 'account',
              start: 0,
              end: 9,
              meaning: 'Account name',
              humanValue: 'alice.bit',
            },
          ],
        },
        heuristicGuesses: [],
      },
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('.bit Account');
    expect(result.current!.displayName).toBe('alice.bit');
    expect(result.current!.href).toBe('/identities/dotbit/0xdotbit_account_id');
  });

  it('returns DID:CKB label via code_hash fallback when no deterministic kind', () => {
    const cell = makeCell({
      type: {
        codeHash: '0x079bb8c1dfb249f60d932f4b1a60fa5cb2a36af3653ac09464f262e2f3f682a9',
        hashType: 'type',
        args: '0xdid_ckb_id',
      },
      dataAnalysis: undefined,
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('DID:CKB Identity');
    expect(result.current!.href).toBe('/identities/did/0xdid_ckb_id');
  });

  it('returns M-NFT token label with token index from segments', () => {
    const cell = makeCell({
      type: { codeHash: '0xmnft_code', hashType: 'type', args: '0xmnft_token_id' },
      dataAnalysis: {
        deterministic: {
          kind: 'mnft_token_cell',
          summary: 'M-NFT Token',
          segments: [
            { label: 'token_index', start: 0, end: 4, meaning: 'Token index', humanValue: '42' },
          ],
        },
        heuristicGuesses: [],
      },
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('M-NFT Token');
    expect(result.current!.displayName).toBe('Token #42');
    expect(result.current!.href).toBe('/objects/mnft/0xmnft_token_id');
  });
});
