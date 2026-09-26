import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import type { Cell } from '@/lib/api';
import { useInventoryLabel } from '@/components/cell/inventory-context';

const API_BASE = '/api/:network/v1';

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
          // The shape `maybe_parse_dotbit_decode` emits: the AccountCell's
          // UTF-8 name tail, data[80..], after the 80-byte fixed header.
          segments: [
            {
              label: 'account',
              start: 80,
              end: 89,
              meaning: 'DAS account name (UTF-8, includes .bit suffix)',
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

  it('returns DID:CKB label from protocolScript', () => {
    const cell = makeCell({
      // Any code hash: recognition comes from the registry slug the API
      // resolved, on either network, never from a hardcoded hash.
      type: { codeHash: '0xany_did_ckb_deployment', hashType: 'type', args: '0xdid_ckb_id' },
      protocolScript: { lock: null, type: 'did-ckb' },
      dataAnalysis: undefined,
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('DID:CKB Identity');
    expect(result.current!.href).toBe('/identities/did/0xdid_ckb_id');
  });

  it('does not recognise did:ckb by code hash alone', () => {
    const cell = makeCell({
      type: {
        codeHash: '0x079bb8c1dfb249f60d932f4b1a60fa5cb2a36af3653ac09464f262e2f3f682a9',
        hashType: 'type',
        args: '0xdid_ckb_id',
      },
      dataAnalysis: undefined,
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).toBeNull();
  });

  // The segment shapes below are what `maybe_parse_dotcell_decode` emits for
  // the real mainnet `support.cell` (M2_OUT1_DATA).
  const SUPPORT_SEGMENTS = [
    { label: 'layout_version', start: 0, end: 1, meaning: 'Layout version (u8)', humanValue: '3' },
    {
      label: 'next_id',
      start: 33,
      end: 53,
      meaning: 'Next name id in the ordered uniqueness ring (zero = end of ring)',
      humanValue: '0x65b5fe7e7070b506f69bd8cabf9e427211106645',
    },
    {
      label: 'expired_at',
      start: 53,
      end: 58,
      meaning: 'Expiry, unix seconds (u40 little-endian)',
      humanValue: '2027-09-21T06:21:18+00:00 (unix 1821507678)',
    },
    {
      label: 'owner_hash20',
      start: 58,
      end: 78,
      meaning: "Owner: first 20 bytes of the owner's lock script hash",
      humanValue: '0x57d926a44d83fc13b21ce037b1e31f4223e3c867',
    },
    {
      label: 'label',
      start: 98,
      end: 105,
      meaning: 'Name label, UTF-8 (empty only on the ring root); id = blake2b(label)[..20]',
      humanValue: 'support.cell · id 0x62d71147ac82b83c8531126cacb0d2f072bfd94a',
    },
  ];

  it('returns .cell name label linking to the identity page', () => {
    const cell = makeCell({
      type: {
        codeHash: '0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54',
        hashType: 'type',
        args: '0xb4f4302965b7d6421481a520ee7eb5971a5e808c',
      },
      protocolScript: { lock: 'dotcell-account-lock', type: 'dotcell-account' },
      dataAnalysis: {
        deterministic: {
          kind: 'dotcell_name',
          summary: 'support.cell name cell (layout v3)',
          segments: SUPPORT_SEGMENTS,
        },
        heuristicGuesses: [],
      },
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('.cell Name');
    expect(result.current!.displayName).toBe('support.cell');
    // The item route accepts a name; the namespace args are not the name id.
    expect(result.current!.href).toBe('/identities/dotcell/support');
    expect(result.current!.summary).toContain('owner 0x57d9...c867');
    expect(result.current!.summary).toContain('expires 2027-09-21');
  });

  it('returns .cell ring root label without an item link', () => {
    const cell = makeCell({
      type: {
        codeHash: '0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54',
        hashType: 'type',
        args: '0xb4f4302965b7d6421481a520ee7eb5971a5e808c',
      },
      protocolScript: { lock: 'dotcell-account-lock', type: 'dotcell-account' },
      dataAnalysis: {
        deterministic: {
          kind: 'dotcell_ring_root',
          summary: '.cell ring root of namespace 0xb4f4…; first name id: end of ring',
          segments: [
            {
              label: 'label',
              start: 98,
              end: 98,
              meaning: 'Name label, UTF-8 (empty only on the ring root); id = blake2b(label)[..20]',
              humanValue: '(empty)',
            },
          ],
        },
        heuristicGuesses: [],
      },
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('.cell Ring Root');
    expect(result.current!.href).toBe('/identities/dotcell');
    expect(result.current!.displayName).toBeNull();
  });

  it('returns .cell label from protocolScript when data analysis is absent', () => {
    const cell = makeCell({
      type: {
        codeHash: '0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54',
        hashType: 'type',
        args: '0xb4f4302965b7d6421481a520ee7eb5971a5e808c',
      },
      protocolScript: { lock: 'dotcell-account-lock', type: 'dotcell-account' },
      dataAnalysis: undefined,
    });

    const { result } = renderHook(() => useInventoryLabel(cell), { wrapper });

    expect(result.current).not.toBeNull();
    expect(result.current!.typeLabel).toBe('.cell Name');
    // Without the label the item cannot be located: link the collection.
    expect(result.current!.href).toBe('/identities/dotcell');
    expect(result.current!.displayName).toBeNull();
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
