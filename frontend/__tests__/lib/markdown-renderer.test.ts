import { beforeEach, describe, expect, it, vi } from 'vitest';
import { parseMarkdownSourcePath } from '@/lib/ai/markdown-route';
import { MarkdownRenderError, renderMarkdownPage } from '@/lib/ai/markdown-renderer';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getActivitySummary24h: vi.fn(),
    getBlocks: vi.fn(),
    getScripts: vi.fn(),
    getScriptFamilyDetail: vi.fn(),
    getScriptUsage: vi.fn(),
    getGlobalActivities: vi.fn(),
    getTransactionDetail: vi.fn(),
    getTransactionLifecycle: vi.fn(),
    getTransactionCellDeps: vi.fn(),
    getMinerAddressDistributionChart: vi.fn(),
    getDotbitItemDetail: vi.fn(),
    getDotbitItemActivities: vi.fn(),
    getDidCkbItemDetail: vi.fn(),
    getDidCkbItemActivities: vi.fn(),
    getBitCellItemDetail: vi.fn(),
    getBitCellItemActivities: vi.fn(),
    getMnftItemDetail: vi.fn(),
    getMnftItemActivities: vi.fn(),
    getNetworkSummary: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

describe('renderMarkdownPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      buildVersion: '0.1.0+feature/foo@abcdef123456',
    };
  });

  it('renders blocks list markdown', async () => {
    vi.mocked(api.getBlocks).mockResolvedValue({
      data: [
        {
          number: 123,
          hash: `0x${'a'.repeat(64)}`,
          parentHash: `0x${'b'.repeat(64)}`,
          timestamp: '2026-02-23T00:00:00Z',
          transactionsCount: 3,
          proposalsCount: 1,
          unclesCount: 0,
          difficulty: '1',
          epoch: '1/10',
          epochNumber: 1,
          epochIndex: 0,
          epochLength: 10,
          nonce: '0x0',
          transactionsRoot: `0x${'c'.repeat(64)}`,
          minerAddress: null,
          minerMessage: null,
          miningReward: null,
          miningRewardTxHash: null,
          hardforkActivation: null,
          compactTarget: '0x0',
          version: 0,
        },
      ],
      total: 1,
      limit: 1,
      hasMore: false,
      nextCursor: null,
    });

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/blocks'),
      searchParams: new URLSearchParams('limit=1'),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('buildVersion: "0.1.0+feature/foo@abcdef123456"');
    expect(result.body).toContain('# Blocks');
    expect(result.body).toContain('| number | hash | txs | proposals | timestamp |');
    expect(result.body).toContain('| 123 |');
  });

  it('renders activities markdown', async () => {
    vi.mocked(api.getActivitySummary24h).mockResolvedValue({
      transferCount: 12,
      daoDepositCount: 3,
      daoWithdrawRequestCount: 1,
      daoWithdrawCompleteCount: 1,
      tokenCount: 4,
      objectCount: 2,
      identityCount: 1,
      scriptCallCount: 5,
      unknownCount: 0,
      coinbaseCount: 1,
      uniqueAddressCount: 9,
      totalCkbMoved: '123000000000',
      scriptCounts: [],
      hoursCovered: 24,
    });
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        {
          txHash: `0x${'a'.repeat(64)}`,
          blockNumber: 123,
          txIndex: 0,
          timestamp: '1700000000',
          isCellbase: false,
          typeCalls: [],
          lockCalls: [],
          protocolActions: [
            { protocol: 'dao', action: 'deposit', metadata: { capacity: '10000000000' } },
          ],
          participants: [
            {
              address: 'ckt1qyq9sampleaddress0000000000000000000000000',
              ckbDelta: '10000000000',
              usedDelta: '0',
              itemDeltas: [],
              tags: 8,
            },
          ],
        },
      ],
      total: 1,
      limit: 1,
      hasMore: false,
      nextCursor: null,
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/activities'),
      searchParams: new URLSearchParams('limit=1&filter=dao'),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Activities');
    expect(result.body).toContain('## Last 24 Hours');
    expect(result.body).toContain('daoDeposit');
    expect(result.body).toContain('daoDepositCount');
    expect(api.getGlobalActivities).toHaveBeenCalledWith({
      limit: 1,
      cursor: undefined,
      filter: 'dao',
    });
  });

  it('fails fast on invalid limit query param', async () => {
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/blocks'),
        searchParams: new URLSearchParams('limit=0'),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<MarkdownRenderError>>({ status: 400 }));
  });

  it('renders miner distribution chart markdown', async () => {
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue({
      title: 'Miner Distribution',
      totalBlocks: 100,
      windowDays: 7,
      fromDate: '2026-07-23',
      toDate: '2026-07-29',
      data: [
        {
          minerLockHash: `0x${'d'.repeat(64)}`,
          address: `0x${'d'.repeat(64)}`,
          minerName: 'ExampleMiner',
          blocksMined: 42,
          percentage: '42%',
        },
      ],
    });

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/charts/miner-address-distribution'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Chart miner-address-distribution');
    expect(result.body).toContain('ExampleMiner');
    expect(result.body).toContain('Total blocks: 100');
  });

  it('renders scripts list markdown with family-level columns', async () => {
    vi.mocked(api.getScripts).mockResolvedValue({
      data: [
        {
          codeHash: 'family:default-lock',
          name: 'Default Lock',
          description: 'Default lock family',
          scriptKind: 'lock',
          rfc: null,
          website: null,
          sourceUrl: null,
          decoderType: null,
          network: '',
          hashType: null,
          dataHash: null,
          typeHash: null,
          tag: null,
          deprecated: false,
          isSystem: false,
          codeCellTxHash: null,
          codeCellOutputIndex: null,
          ownedCapacitySum: '1500',
          ownedKnowledgeSum: '900',
          liveCellsCount: 10,
          cellsCount: 14,
        },
      ],
      total: 1,
      limit: 1,
      hasMore: false,
      nextCursor: null,
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/scripts'),
      searchParams: new URLSearchParams('limit=1'),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Scripts');
    expect(result.body).toContain(
      '| name | scriptKind | liveCellsCount | cellsCount | ownedCapacitySum |'
    );
    expect(result.body).toContain('| Default Lock | lock | 10 | 14 | 1500 |');
    expect(result.body).not.toContain('family:default-lock');
    expect(result.body).not.toContain('| network |');
  });

  it('renders script detail markdown with family, versions, deployments, and observed references', async () => {
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue({
      familyId: 'default-lock',
      name: 'Default Lock',
      description: 'Default lock family',
      scriptKind: 'lock',
      website: null,
      liveCellsCount: 10,
      cellsCount: 14,
      ownedCapacitySum: '1500',
      ownedKnowledgeSum: '900',
      versionsCount: 1,
      versions: [
        {
          versionHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
          name: 'Default Lock',
          description: 'Default lock family',
          scriptKind: 'lock',
          website: null,
          deprecated: false,
          canonicalReferenceHash:
            '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          canonicalHashType: 'type',
          deployedAt: 1700000000000,
          liveCellsCount: 10,
          cellsCount: 14,
          ownedCapacitySum: '1500',
          ownedKnowledgeSum: '900',
          codeCellsLiveCount: 1,
          codeCellsTotal: 1,
          deployments: [
            {
              hashType: 'type',
              typeReferenceHash:
                '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
              dataReferenceHash:
                '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
              codeCellTxHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
              codeCellOutputIndex: 0,
              deployedAt: 1700000000000,
            },
          ],
          references: [
            {
              referenceHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
              hashType: 'data1',
              liveCellsCount: 6,
              cellsCount: 8,
              ownedCapacitySum: '800',
              ownedKnowledgeSum: '500',
            },
          ],
        },
      ],
    } as any);
    vi.mocked(api.getScriptUsage).mockResolvedValue({
      name: 'Default Lock',
      cellsCount: 14,
      liveCellsCount: 10,
      capacitySum: '1500',
      ownedCapacitySum: '1500',
      commonKnowledgeSizeSum: '900',
      ownedKnowledgeSum: '900',
      byDeployment: [
        {
          codeHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
          scriptKind: 'lock',
          cellsCount: 14,
          liveCellsCount: 10,
          capacitySum: '1500',
          ownedCapacitySum: '1500',
          commonKnowledgeSizeSum: '900',
          ownedKnowledgeSum: '900',
        },
      ],
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/scripts/Default%20Lock'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Script Default Lock');
    expect(result.body).toContain('## Family');
    expect(result.body).toContain('default-lock');
    expect(result.body).toContain('## Versions');
    expect(result.body).toContain('## Deployments');
    expect(result.body).toContain('## Observed References');
    expect(result.body).toContain('data1');
    expect(result.body).not.toContain('family:default-lock');
    expect(result.body).not.toContain('| isSystem |');
  });

  it('returns 404 for unknown chart slug', async () => {
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/charts/not-exists'),
        searchParams: new URLSearchParams(),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<MarkdownRenderError>>({ status: 404 }));
  });

  it('returns 404 for removed cell age chart slug', async () => {
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/charts/cell-age-vs-used-capacity'),
        searchParams: new URLSearchParams(),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<MarkdownRenderError>>({ status: 404 }));
  });

  it('renders mnft item detail markdown', async () => {
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      nftId: '0xmnft',
      standard: 'm-nft',
      isLive: true,
      ownerLockHash: '0xowner',
      createdAtBlock: 123,
      tokenIndex: 9,
      characteristicHex: '0x0102',
      configure: 1,
      state: 0,
      txHash: '0xtx',
      outputIndex: 3,
      class: {
        classId: '0xclass',
        issuerId: '0xissuer',
        name: 'Class A',
        description: 'desc',
        renderer: 'renderer:v1',
        total: 100,
        issued: 9,
        configure: 1,
      },
      issuer: {
        issuerId: '0xissuer',
        name: 'Issuer A',
        classCount: 2,
        setCount: 3,
        infoHex: '0x7b7d',
      },
      lifecycle: [],
    } as any);
    vi.mocked(api.getMnftItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xtx',
          blockNumber: 123,
          txIndex: 0,
          timestamp: '1700000000',
          actions: ['transfer'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/objects/mnft/0xmnft'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# mNFT 0xmnft');
    expect(result.body).toContain('## Token');
    expect(result.body).toContain('Class A');
    expect(result.body).toContain('Issuer A');
    expect(result.body).toContain('## Activities');
    expect(result.body).toContain('transfer');
    expect(api.getMnftItemActivities).toHaveBeenCalledWith('0xmnft', {
      limit: 20,
      cursor: undefined,
      action: undefined,
    });
  });

  it('fails fast on invalid mnft activity action query param', async () => {
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/objects/mnft/0xmnft'),
        searchParams: new URLSearchParams('action=invalid'),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<MarkdownRenderError>>({ status: 400 }));
  });

  it('renders dotbit item detail markdown', async () => {
    vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
      nftId: '0xdotbit',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xowner',
      isLive: false,
      createdAtBlock: 456,
      expiredAt: 1800000000,
      txHash: null,
      outputIndex: null,
    } as any);
    vi.mocked(api.getDotbitItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xtx',
          blockNumber: 456,
          txIndex: 0,
          timestamp: '1700000000',
          actions: ['transfer'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/identities/dotbit/0xdotbit'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# .bit alice.bit');
    expect(result.body).toContain('## Account');
    expect(result.body).toContain('## Activities');
    expect(result.body).toContain('transfer');
    expect(api.getDotbitItemActivities).toHaveBeenCalledWith('0xdotbit', {
      limit: 20,
      cursor: undefined,
      action: undefined,
    });
  });

  it('renders did:ckb item detail markdown', async () => {
    vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
      nftId: '0xdid',
      name: 'did:alice.ckb',
      standard: 'did_ckb',
      ownerLockHash: '0xowner',
      isLive: false,
      createdAtBlock: 456,
      txHash: null,
      outputIndex: null,
    } as any);
    vi.mocked(api.getDidCkbItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xtx',
          blockNumber: 456,
          txIndex: 0,
          timestamp: '1700000000',
          actions: ['transfer'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/identities/did/0xdid'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# did:ckb did:alice.ckb');
    expect(result.body).toContain('## Identity');
    expect(result.body).toContain('## Activities');
    expect(result.body).toContain('transfer');
    expect(api.getDidCkbItemActivities).toHaveBeenCalledWith('0xdid', {
      limit: 20,
      cursor: undefined,
      action: undefined,
    });
  });

  it('renders .bit Cell item detail markdown', async () => {
    vi.mocked(api.getBitCellItemDetail).mockResolvedValue({
      nftId: '0xbitcell',
      name: 'alice.bit-cell',
      standard: 'bit_cell',
      ownerLockHash: '0xowner',
      isLive: true,
      createdAtBlock: 456,
      expiredAt: 1800000000,
      txHash: '0xtx',
      outputIndex: 3,
    } as any);
    vi.mocked(api.getBitCellItemActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xtx',
          blockNumber: 456,
          txIndex: 0,
          timestamp: '1700000000',
          actions: ['mint'],
        },
      ],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    } as any);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/identities/bit-cell/0xbitcell'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# .bit Cell alice.bit-cell');
    expect(result.body).toContain('## Identity');
    expect(result.body).toContain('expiredAt');
    expect(result.body).toContain('## Activities');
    expect(api.getBitCellItemActivities).toHaveBeenCalledWith('0xbitcell', {
      limit: 20,
      cursor: undefined,
      action: undefined,
    });
  });

  it('renders peers markdown with the reachability caveat when the crawler is off', async () => {
    vi.mocked(api.getNetworkSummary).mockResolvedValue({
      enabled: false,
      hasData: false,
      lastRound: null,
      activeRound: null,
    });

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/network'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Peers');
    expect(result.body).toContain('advertised candidate is not verified');
    expect(result.body).toContain('Crawler disabled');
    expect(result.body).toContain('/network/summary');
  });

  it('renders peers markdown with last-round stats when crawl data exists', async () => {
    vi.mocked(api.getNetworkSummary).mockResolvedValue({
      enabled: true,
      hasData: true,
      lastRound: {
        roundId: 5,
        startedAt: 0,
        finishedAt: 1700000000,
        candidatePeers: 2,
        verifiedRetainedPeers: 2,
        reachablePeers: 1,
        verifiedUnavailablePeers: 1,
        exhaustedCandidates: 1,
        addressAttempts: 2,
        nonSuccessfulAddressAttempts: 1,
        foreignPeers: 0,
        malformedAddresses: 0,
        newVerifiedPeers: 0,
        peerOutcomes: {
          sameNetworkIdentified: 1,
          exhausted: { withRetainedVerification: 1, withoutRetainedVerification: 0 },
          foreignNetwork: { withRetainedVerification: 0, withoutRetainedVerification: 0 },
        },
        addressObservations: {
          dialRequestFailed: 1,
          noAuthenticatedSessionBeforeDeadline: 0,
          authenticatedSessionWithoutIdentifyBeforeDeadline: 0,
          malformedIdentify: 0,
          foreignNetwork: 0,
          sameNetworkIdentified: 1,
        },
        discovery: {
          validNodesMessages: 1,
          malformedMessages: 0,
          unexpectedMessages: 0,
          normalizedAdvertisedAddresses: 2,
          rejectedAdvertisedAddresses: 0,
        },
      },
      activeRound: null,
    });

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/network'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('## Last Round');
    expect(result.body).toContain('reachablePeers');
    expect(result.body).toContain('| verifiedRetainedPeers | 2 |');
    expect(result.body).not.toContain('totalKnown');
  });

  it('renders tx detail markdown with witness summary', async () => {
    const hash = `0x${'1'.repeat(64)}`;
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      hash,
      status: 'committed',
      pendingSince: null,
      blockNumber: 100,
      blockHash: `0x${'2'.repeat(64)}`,
      index: 0,
      inputsCount: 1,
      outputsCount: 1,
      fee: '1000',
      feeRate: '500',
      txSize: 222,
      cycles: 333,
      isCellbase: false,
      timestamp: '2026-02-20T16:46:12Z',
      confirmations: 4,
      inputsCapacity: '10000000000',
      outputsCapacity: '9999999000',
      inputsCommonKnowledgeSize: '100',
      outputsCommonKnowledgeSize: '90',
      inputs: [],
      outputs: [],
      witnesses: ['0x1b00000010000000160000001600000006000000112205000000aa', '0x64617301020304'],
      witnessesAvailable: true,
    } as any);
    vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
      hash,
      phase: 'committed',
      proposalId: '0x01',
      proposedIn: null,
      committedIn: null,
      commitmentDistance: null,
      commitmentWindow: { close: 2, far: 10 },
      isCellbase: false,
      confirmations: 4,
    } as any);
    vi.mocked(api.getTransactionCellDeps).mockResolvedValue([]);

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath(`/tx/${hash}`),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Transaction');
    expect(result.body).toContain('## Witnesses');
    expect(result.body).toContain('## Witness Inference');
    expect(result.body).toContain('WitnessArgs');
    expect(result.body).toContain('DASWitness');
    expect(result.body).toContain('extra_witnesses');
    expect(result.body).toContain('| index | role | bytes | deterministicKind | heuristics |');
  });
});
