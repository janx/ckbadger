import { http, HttpResponse } from 'msw';

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001/api/v1';

export const handlers = [
  http.get(`${API_BASE}/blocks`, ({ request }) => {
    const url = new URL(request.url);
    const limit = parseInt(url.searchParams.get('limit') || '20');

    return HttpResponse.json({
      data: [
        {
          number: 1000000,
          hash: '0xabc123def456789012345678901234567890123456789012345678901234abcd',
          parentHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
          timestamp: '2024-01-15T10:30:00Z',
          transactionsCount: 5,
          proposalsCount: 0,
          unclesCount: 0,
          difficulty: '0x1000000',
          epoch: '0x1',
          epochNumber: 100,
          epochIndex: 50,
          epochLength: 1000,
          nonce: '0x0',
          transactionsRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
          minerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
          minerMessage: null,
          miningReward: '1000000000',
          miningRewardTxHash: '0x123...',
          compactTarget: '0x1a000000',
          version: 0,
        },
        {
          number: 999999,
          hash: '0xdef456789012345678901234567890123456789012345678901234567890efgh',
          parentHash: '0xabc123def456789012345678901234567890123456789012345678901234abcd',
          timestamp: '2024-01-15T10:29:50Z',
          transactionsCount: 3,
          proposalsCount: 0,
          unclesCount: 0,
          difficulty: '0x1000000',
          epoch: '0x1',
          epochNumber: 100,
          epochIndex: 49,
          epochLength: 1000,
          nonce: '0x0',
          transactionsRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
          minerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
          minerMessage: null,
          miningReward: '1000000000',
          miningRewardTxHash: '0x456...',
          compactTarget: '0x1a000000',
          version: 0,
        },
      ],
      total: 1000000,
      limit,
      hasMore: true,
      nextCursor: '999998',
    });
  }),

  http.get(`${API_BASE}/blocks/:id`, ({ params }) => {
    const { id } = params;
    return HttpResponse.json({
      number: parseInt(id as string) || 1000000,
      hash: '0xabc123def456789012345678901234567890123456789012345678901234abcd',
      parentHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
      timestamp: '2024-01-15T10:30:00Z',
      transactionsCount: 5,
      proposalsCount: 0,
      unclesCount: 0,
      difficulty: '0x1000000',
      epoch: '0x1',
      epochNumber: 100,
      epochIndex: 50,
      epochLength: 1000,
      nonce: '0x0',
      transactionsRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
      minerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
      minerMessage: null,
      miningReward: '1000000000',
      miningRewardTxHash: '0x123...',
      compactTarget: '0x1a000000',
      version: 0,
    });
  }),

  http.get(`${API_BASE}/transactions`, () => {
    return HttpResponse.json({
      data: [
        {
          hash: '0xtx123456789012345678901234567890123456789012345678901234567890ab',
          blockNumber: 1000000,
          blockHash: '0xabc123...',
          index: 0,
          inputsCount: 2,
          outputsCount: 3,
          fee: '100000',
          isCellbase: false,
          timestamp: '2024-01-15T10:30:00Z',
        },
      ],
      total: 5000000,
      limit: 20,
      hasMore: true,
      nextCursor: '999999',
    });
  }),

  http.get(`${API_BASE}/statistics/network`, () => {
    return HttpResponse.json({
      latestBlock: 1000000,
      avgBlockTime: '8.5',
      hashRate: '1.5 PH/s',
      difficulty: '0x1000000000',
      epoch: '100/50/1000',
      tps: '2.5',
      estimatedEpochTime: '4h 30m',
      transactionsPerMinute: '150',
      transactionsPerDay: '216000',
      syncStatus: {
        isSyncing: false,
        syncedBlock: 1000000,
        tipBlock: 1000000,
        progress: 100,
        estimatedTime: null,
        chartDataMayBeIncomplete: false,
      },
      deepForkStatus: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: null,
        chainTip: null,
        forkPoint: null,
      },
    });
  }),

  http.get(`${API_BASE}/status/health`, () => {
    return HttpResponse.json({ status: 'ok' });
  }),

  http.get(`${API_BASE}/forks`, () => {
    return HttpResponse.json({
      data: [
        {
          id: 1,
          eventType: 'auto',
          depth: 3,
          forkPointNumber: 1000,
          forkPointHash: '0xabc123',
          oldTipNumber: 1003,
          oldTipHash: '0xdef456',
          newTipNumber: 1004,
          newTipHash: '0x789abc',
          orphanedBlocksCount: 3,
          orphanedTxsCount: 15,
          detectedAt: '2024-01-15T10:30:00Z',
          resolvedAt: null,
          resolvedBy: null,
          resolutionAction: null,
          resolutionNotes: null,
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });
  }),

  http.get(`${API_BASE}/forks/recent`, () => {
    return HttpResponse.json({
      hasRecentReorg: false,
      reorg: null,
      deepFork: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: null,
        chainTip: null,
        forkPoint: null,
      },
    });
  }),

  http.get(`${API_BASE}/forks/:id`, ({ params }) => {
    const { id } = params;
    return HttpResponse.json({
      event: {
        id: parseInt(id as string),
        eventType: 'auto',
        depth: 3,
        forkPointNumber: 1000,
        forkPointHash: '0xabc123',
        oldTipNumber: 1003,
        oldTipHash: '0xdef456',
        newTipNumber: 1004,
        newTipHash: '0x789abc',
        orphanedBlocksCount: 3,
        orphanedTxsCount: 15,
        detectedAt: '2024-01-15T10:30:00Z',
        resolvedAt: null,
        resolvedBy: null,
        resolutionAction: null,
        resolutionNotes: null,
      },
      orphanedBlocks: [
        {
          number: 1001,
          hash: '0x111',
          parentHash: '0x000',
          timestamp: '2024-01-15T10:28:00Z',
          transactionsCount: 5,
          minerLockHash: null,
        },
      ],
      orphanedTransactions: [
        {
          hash: '0xaaa',
          blockNumber: 1001,
          blockHash: '0x111',
          txIndex: 0,
          inputsCount: 2,
          outputsCount: 3,
          totalCapacity: '100000000000',
        },
      ],
    });
  }),

  http.get(`${API_BASE}/addresses/:addr/asset-transfers`, () => {
    return HttpResponse.json({
      data: [],
      total: 0,
      limit: 100,
      hasMore: false,
      nextCursor: null,
    });
  }),

  http.get(`${API_BASE}/transactions/:hash/asset-transfers`, () => {
    return HttpResponse.json([]);
  }),

  http.get(`${API_BASE}/dao/summary/:lockHash`, () => {
    return HttpResponse.json({
      hasDaoActivity: false,
      activeDepositsCount: 0,
      pendingWithdrawalsCount: 0,
      completedWithdrawalsCount: 0,
      totalLockedCapacity: '0',
      totalLockedCkb: '0',
      unclaimedCompensation: '0',
      unclaimedCompensationCkb: '0',
      totalCompensationEarned: '0',
      totalCompensationEarnedCkb: '0',
      estimatedApc: '',
    });
  }),

  http.get(`${API_BASE}/dao/deposits/:lockHash`, () => {
    return HttpResponse.json({
      data: [],
      total: 0,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });
  }),
];
