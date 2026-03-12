import { beforeEach, describe, expect, it, vi } from 'vitest';
import { parseRawSourcePath } from '@/lib/ai/raw-route';
import { RawRenderError, renderRawPage } from '@/lib/ai/raw-renderer';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getBlock: vi.fn(),
    getCell: vi.fn(),
    getDotbitItemActivities: vi.fn(),
    getDotbitItemDetail: vi.fn(),
    getDidCkbItemActivities: vi.fn(),
    getDidCkbItemDetail: vi.fn(),
    getMnftItemActivities: vi.fn(),
    getMnftItemDetail: vi.fn(),
    getTransactionDetail: vi.fn(),
    getTransactionCellDeps: vi.fn(),
    getTransactionLifecycle: vi.fn(),
  },
}));

describe('renderRawPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.unstubAllGlobals();
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      buildVersion: '0.1.0+feature/foo@abcdef123456',
    };
  });

  it('renders tx raw with default profile', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      hash: `0x${'a'.repeat(64)}`,
      status: 'committed',
      pendingSince: null,
      blockNumber: 123,
      blockHash: `0x${'b'.repeat(64)}`,
      index: 0,
      inputsCount: 1,
      outputsCount: 2,
      fee: '1000',
      confirmations: 10,
      isCellbase: false,
      timestamp: '2026-02-23T00:00:00Z',
      inputsCapacity: '100',
      outputsCapacity: '99',
      inputsUsedCapacity: '50',
      outputsUsedCapacity: '49',
      inputs: [
        {
          lock: {
            codeHash: `0x${'1'.repeat(64)}`,
            hashType: 'type',
            args: '0x01',
          },
        },
      ],
      outputs: [],
      witnesses: ['0x1b00000010000000160000001600000006000000112205000000aa'],
      witnessesAvailable: true,
    });

    const result = await renderRawPage({
      page: parseRawSourcePath(`/tx/0x${'a'.repeat(64)}`),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body.meta.format).toBe('raw');
    expect(result.body.meta.profile).toBe('default');
    expect(result.body.meta.schemaVersion).toBe('1.1.0');
    expect(result.body.meta.buildVersion).toBe('0.1.0+feature/foo@abcdef123456');
    expect(result.body.data?.transaction?.hash).toBe(`0x${'a'.repeat(64)}`);
    expect(result.body.data?.txWitness?.available).toBe(true);
    expect(result.body.data?.txWitness?.witnessesCount).toBe(1);
    expect(result.body.data?.txWitness?.analyses[0]?.deterministic?.kind).toBe('WitnessArgs');
    expect(
      result.body.data?.txWitness?.inference.some((item) => item.kind === 'input_witness_coverage')
    ).toBe(true);
  });

  it('renders tx raw with debugger profile', async () => {
    const txHash = `0x${'a'.repeat(64)}`;
    const inputTxHash = `0x${'1'.repeat(64)}`;
    const depTxHash = `0x${'2'.repeat(64)}`;
    const headerHash = `0x${'3'.repeat(64)}`;

    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      hash: txHash,
      status: 'committed',
      pendingSince: null,
      blockNumber: 123,
      blockHash: `0x${'b'.repeat(64)}`,
      index: 0,
      inputsCount: 1,
      outputsCount: 2,
      fee: '1000',
      confirmations: 10,
      isCellbase: false,
      timestamp: '2026-02-23T00:00:00Z',
      inputsCapacity: '100',
      outputsCapacity: '99',
      inputsUsedCapacity: '50',
      outputsUsedCapacity: '49',
      inputs: [
        {
          lock: {
            codeHash: `0x${'1'.repeat(64)}`,
            hashType: 'type',
            args: '0x01',
          },
        },
      ],
      outputs: [],
      witnesses: ['0x1b00000010000000160000001600000006000000112205000000aa', '0x64617301020304'],
      witnessesAvailable: true,
    });
    vi.mocked(api.getTransactionCellDeps).mockResolvedValue([
      {
        outPointTxHash: `0x${'c'.repeat(64)}`,
        outPointIndex: 0,
        depType: 'code',
      },
    ]);
    vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
      hash: txHash,
      phase: 'committed',
      proposalId: `0x${'d'.repeat(20)}`,
      proposedIn: null,
      committedIn: {
        blockNumber: 123,
        blockHash: `0x${'b'.repeat(64)}`,
        timestamp: '2026-02-23T00:00:00Z',
      },
      commitmentDistance: 0,
      commitmentWindow: { close: 2, far: 10 },
      isCellbase: false,
      confirmations: 10,
    });
    const rpcFetch = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const payload = JSON.parse(String(init?.body ?? '{}')) as {
        method?: string;
        params?: unknown[];
      };

      if (payload.method === 'get_transaction') {
        expect(payload.params?.[0]).toBe(txHash);
        return new Response(
          JSON.stringify({
            jsonrpc: '2.0',
            id: 1,
            result: {
              transaction: {
                version: '0x0',
                cell_deps: [{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' }],
                header_deps: [headerHash],
                inputs: [{ previous_output: { tx_hash: inputTxHash, index: '0x1' }, since: '0x0' }],
                outputs: [
                  {
                    capacity: '0x174876e800',
                    lock: {
                      code_hash: `0x${'e'.repeat(64)}`,
                      hash_type: 'type',
                      args: '0x',
                    },
                    type: null,
                  },
                ],
                outputs_data: ['0x'],
                witnesses: ['0x'],
              },
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        );
      }

      if (payload.method === 'get_live_cell') {
        const outPoint = payload.params?.[0] as { tx_hash: string; index: string };
        if (outPoint.tx_hash === inputTxHash && outPoint.index === '0x1') {
          return new Response(
            JSON.stringify({
              jsonrpc: '2.0',
              id: 1,
              result: {
                status: 'live',
                cell: {
                  output: {
                    capacity: '0x174876e800',
                    lock: {
                      code_hash: `0x${'f'.repeat(64)}`,
                      hash_type: 'type',
                      args: '0x',
                    },
                    type: null,
                  },
                  data: { content: '0x' },
                },
              },
            }),
            { status: 200, headers: { 'content-type': 'application/json' } }
          );
        }
        if (outPoint.tx_hash === depTxHash && outPoint.index === '0x0') {
          return new Response(
            JSON.stringify({
              jsonrpc: '2.0',
              id: 1,
              result: {
                status: 'live',
                cell: {
                  output: {
                    capacity: '0x174876e800',
                    lock: {
                      code_hash: `0x${'9'.repeat(64)}`,
                      hash_type: 'type',
                      args: '0x',
                    },
                    type: null,
                  },
                  data: { content: '0x' },
                },
              },
            }),
            { status: 200, headers: { 'content-type': 'application/json' } }
          );
        }
      }

      if (payload.method === 'get_header') {
        expect(payload.params?.[0]).toBe(headerHash);
        return new Response(
          JSON.stringify({
            jsonrpc: '2.0',
            id: 1,
            result: {
              compact_target: '0x0',
              hash: headerHash,
              number: '0x1',
              parent_hash: `0x${'a'.repeat(64)}`,
              nonce: '0x0',
              timestamp: '0x0',
              transactions_root: `0x${'b'.repeat(64)}`,
              proposals_hash: `0x${'c'.repeat(64)}`,
              extra_hash: null,
              uncles_hash: null,
              version: '0x0',
              epoch: '0x0',
              dao: `0x${'d'.repeat(64)}`,
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        );
      }

      throw new Error(`Unexpected RPC call: ${payload.method}`);
    });
    vi.stubGlobal('fetch', rpcFetch);

    const result = await renderRawPage({
      page: parseRawSourcePath(`/tx/${txHash}`),
      searchParams: new URLSearchParams('profile=debugger'),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body.meta.profile).toBe('debugger');
    expect(result.body.data?.txDebugger?.debugger.directRunnable).toBe(true);
    expect(result.body.data?.txDebugger?.mockTransaction.mock_info.inputs).toHaveLength(1);
    expect(result.body.data?.txDebugger?.mockTransaction.mock_info.cell_deps).toHaveLength(1);
    expect(result.body.data?.txDebugger?.debugger.rpcUrl).toContain('8114');
    expect(result.body.data?.txWitness?.witnessesCount).toBe(2);
    expect(
      result.body.data?.txWitness?.inference.some((item) => item.kind === 'extra_witnesses')
    ).toBe(true);
    expect(rpcFetch).toHaveBeenCalled();
  });

  it('renders did:ckb item raw with default profile', async () => {
    vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
      nftId: '0xdid',
      name: 'alice.did',
      standard: 'did_ckb',
      ownerLockHash: '0xowner',
      isLive: true,
      createdAtBlock: 123,
      txHash: '0xtx',
      outputIndex: 0,
      expiredAt: null,
    });
    vi.mocked(api.getDidCkbItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xact',
          blockNumber: 124,
          txIndex: 0,
          timestamp: '2026-02-23T00:00:00Z',
          actions: ['transfer'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    const result = await renderRawPage({
      page: parseRawSourcePath('/identities/did/0xdid'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body.meta.profile).toBe('default');
    expect(result.body.data?.didCkbItem?.nftId).toBe('0xdid');
    expect(result.body.data?.didCkbActivities?.data[0]?.actions).toEqual(['transfer']);
    expect(api.getDidCkbItemDetail).toHaveBeenCalledWith('0xdid');
    expect(api.getDidCkbItemActivities).toHaveBeenCalledWith('0xdid', {
      action: undefined,
      cursor: undefined,
      limit: 20,
    });
  });

  it('renders dotbit item raw with default profile', async () => {
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xdotbit',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xowner',
      isLive: true,
      createdAtBlock: 321,
      txHash: '0xtx',
      outputIndex: 1,
      expiredAt: 1_800_000_000,
    });
    vi.mocked(api.getDotbitItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xactdot',
          blockNumber: 322,
          txIndex: 0,
          timestamp: '2026-02-23T00:00:00Z',
          actions: ['mint'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    const result = await renderRawPage({
      page: parseRawSourcePath('/identities/dotbit/0xdotbit'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body.meta.profile).toBe('default');
    expect(result.body.data?.dotbitItem?.name).toBe('alice.bit');
    expect(result.body.data?.dotbitActivities?.data[0]?.actions).toEqual(['mint']);
    expect(api.getDotbitItemDetail).toHaveBeenCalledWith('0xdotbit');
    expect(api.getDotbitItemActivities).toHaveBeenCalledWith('0xdotbit', {
      action: undefined,
      cursor: undefined,
      limit: 20,
    });
  });

  it('renders mnft item raw with default profile', async () => {
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      nftId: '0xmnft',
      standard: 'm_nft',
      isLive: true,
      ownerLockHash: '0xowner',
      createdAtBlock: 500,
      tokenIndex: 12,
      characteristicHex: '0x1234',
      configure: 1,
      state: 0,
      txHash: '0xtx',
      outputIndex: 2,
      class: {
        classId: '0xclass',
        issuerId: '0xissuer',
        name: 'Class A',
        description: null,
        renderer: null,
        total: 100,
        issued: 10,
        configure: 1,
      },
      issuer: {
        issuerId: '0xissuer',
        name: 'Issuer A',
        classCount: 1,
        setCount: 10,
        infoHex: null,
      },
      lifecycle: [],
    });
    vi.mocked(api.getMnftItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xactmnft',
          blockNumber: 501,
          txIndex: 1,
          timestamp: '2026-02-23T00:00:00Z',
          actions: ['transfer'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    const result = await renderRawPage({
      page: parseRawSourcePath('/objects/mnft/0xmnft'),
      searchParams: new URLSearchParams('action=transfer'),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body.meta.profile).toBe('default');
    expect(result.body.data?.mnftItem?.nftId).toBe('0xmnft');
    expect(result.body.data?.mnftActivities?.data[0]?.actions).toEqual(['transfer']);
    expect(api.getMnftItemDetail).toHaveBeenCalledWith('0xmnft');
    expect(api.getMnftItemActivities).toHaveBeenCalledWith('0xmnft', {
      action: 'transfer',
      cursor: undefined,
      limit: 20,
    });
  });

  it('fails fast on unsupported profile', async () => {
    await expect(
      renderRawPage({
        page: parseRawSourcePath('/blocks/123'),
        searchParams: new URLSearchParams('profile=debugger'),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<RawRenderError>>({ status: 400 }));
  });

  it('returns 404 body for unknown route', async () => {
    const result = await renderRawPage({
      page: parseRawSourcePath('/charts/hash-rate'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(404);
    expect(result.body.error?.code).toBe('unknown_page');
  });
});
