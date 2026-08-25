import { http, HttpResponse } from 'msw';

// API calls are now same-origin and network-scoped (`/api/<network>/v1/*`), served by the
// Rust proxy in production. A relative handler base with a `:network` path param matches the
// relative fetch regardless of which network the active URL selects (MSW resolves the
// relative path against jsdom's location).
const API_BASE = '/api/:network/v1';

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
        blocksPerSecond: null,
        emaBlocksPerSecond: null,
      },
      deepForkStatus: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: null,
        chainTip: null,
        forkPoint: null,
      },
      knowledgeSize: null,
      circulatingSupply: null,
      daoLocked: null,
    });
  }),

  http.get(`${API_BASE}/status/health`, () => {
    return HttpResponse.json({ status: 'ok' });
  }),

  http.get(`${API_BASE}/status`, () => {
    return HttpResponse.json({
      sync: {
        isSyncing: false,
        syncedBlock: 1000000,
        tipBlock: 1000000,
        progress: 100,
        estimatedTime: null,
        lastSyncedAt: '2024-01-15T10:30:00Z',
        chartDataMayBeIncomplete: false,
      },
      integrity: {
        isRunning: false,
        pendingCount: 0,
        totalCount: 1000,
        processedCount: 1000,
        progress: 100,
        estimatedTime: null,
        startedAt: null,
        lastCheckAt: '2024-01-15T10:30:00Z',
        missingCyclesCount: 0,
        recentFixes: [],
      },
      labelImport: {
        isRunning: false,
        tokenTotalCount: 100,
        tokenImportedCount: 100,
        scriptTotalCount: 50,
        scriptImportedCount: 50,
        progress: 100,
        startedAt: null,
        lastCheckAt: '2024-01-15T10:30:00Z',
      },
      indexRebuild: {
        isRebuilding: false,
        total: 28,
        completed: 28,
        currentIndex: null,
        failed: [],
        progress: 100,
        startedAt: null,
      },
    });
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

  http.get(`${API_BASE}/charts/hodl-wave`, () => {
    return HttpResponse.json({
      title: 'CKB HODL Wave',
      data: [
        {
          date: '2024-01-01',
          values: {
            '24h': '5.00',
            '1d1w': '10.00',
            '1w1m': '15.00',
            '1m3m': '10.00',
            '3m6m': '10.00',
            '6m1y': '15.00',
            '1y3y': '20.00',
            gt3y: '15.00',
            holderCount: '42000',
          },
        },
      ],
      series: [
        { key: '24h', label: '24h', color: '#6366f1' },
        { key: '1d1w', label: '1d-1w', color: '#4ade80' },
        { key: '1w1m', label: '1w-1m', color: '#f87171' },
        { key: '1m3m', label: '1m-3m', color: '#f59e0b' },
        { key: '3m6m', label: '3m-6m', color: '#d4e157' },
        { key: '6m1y', label: '6m-1y', color: '#22c55e' },
        { key: '1y3y', label: '1y-3y', color: '#67e8f9' },
        { key: 'gt3y', label: '> 3y', color: '#a78bfa' },
      ],
    });
  }),

  // --- Inventory context mock handlers ---

  http.get(`${API_BASE}/spore/objects/:sporeId`, () => {
    return HttpResponse.json({
      sporeId: '0xspore123',
      txHash: '0xabc123',
      outputIndex: 0,
      clusterId: '0xcluster456',
      contentType: 'image/png',
      contentSize: 1024,
      ownerLockHash: '0xownerhash789',
      ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xws',
      isLive: true,
      createdAtBlock: 100000,
      ownedCapacity: '14500000000',
      ownedKnowledge: null,
      mediaProfile: {
        tier: 'pure_ckb',
        sources: [],
        issues: [],
      },
    });
  }),

  http.get(`${API_BASE}/spore/clusters/:clusterId`, () => {
    return HttpResponse.json({
      clusterId: '0xcluster456',
      name: 'Test Cluster',
      description: 'A test cluster',
      ownerLockHash: '0xownerhash789',
      ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xws',
      sporesCount: 42,
      holdersCount: 15,
      activitiesCount: 100,
      createdAtBlock: 90000,
    });
  }),

  http.get(`${API_BASE}/tokens/:typeHash`, () => {
    return HttpResponse.json({
      typeScriptHash: '0xtokenhash123',
      typeCodeHash: '0xcode123',
      typeHashType: 'type',
      typeArgs: '0xargs123',
      standard: 'xudt',
      name: 'Test Token',
      symbol: 'TT',
      decimals: 8,
      description: 'A test token',
      iconUrl: null,
      published: true,
      famous: false,
      tags: null,
      udtType: null,
      manager: null,
      email: null,
      operatorWebsite: null,
      totalSupply: '1000000000000000000',
      maximumSupply: null,
      maximumSupplyStatus: 'unlimited',
      holdersCount: 500,
      transfersCount: 10000,
      transfers24h: 50,
      cellsCount: 200,
      totalCapacity: null,
      totalCommonKnowledgeSize: null,
    });
  }),

  http.get(`${API_BASE}/assets/objects/items/:nftId`, () => {
    return HttpResponse.json({
      nftId: '0xmnft_token_id',
      standard: 'mnft',
      isLive: true,
      ownerLockHash: '0xownerhash789',
      createdAtBlock: 100000,
      tokenIndex: 0,
      characteristicHex: '0x0102030405060708',
      configure: 0,
      state: 0,
      txHash: '0xabc123',
      outputIndex: 0,
      class: {
        classId: '0xclass123',
        issuerId: '0xissuer123',
        name: 'Test Object Class',
        description: 'A test class',
        renderer: null,
        total: 100,
        issued: 42,
        configure: 0,
      },
      issuer: {
        issuerId: '0xissuer123',
        name: 'Test Issuer',
        classCount: 5,
        setCount: 2,
        infoHex: null,
      },
      lifecycle: [],
    });
  }),

  http.get(`${API_BASE}/assets/objects/:collectionId`, () => {
    return HttpResponse.json({
      collectionId: '0xclass123',
      standard: 'mnft',
      name: 'Test Collection',
      totalCount: 100,
      liveCount: 90,
      holdersCount: 30,
      activitiesCount: 200,
      ownedCapacity: '50000000000',
      ownedKnowledge: '40000000000',
      classDetail: {
        classId: '0xclass123',
        issuerId: '0xissuer123',
        name: 'Test Object Class',
        description: 'A test class',
        renderer: null,
        total: 100,
        issued: 42,
        configure: 0,
      },
      issuerDetail: {
        issuerId: '0xissuer123',
        name: 'Test Issuer',
        classCount: 5,
        setCount: 2,
        infoHex: null,
      },
    });
  }),

  http.get(`${API_BASE}/assets/identities/dotbit/items/:nftId`, () => {
    return HttpResponse.json({
      nftId: '0xdotbit_account_id',
      name: 'alice.bit',
      standard: 'dotbit',
      ownerLockHash: '0xownerhash789',
      isLive: true,
      createdAtBlock: 100000,
      expiredAt: 1750000000,
    });
  }),

  http.get(`${API_BASE}/assets/identities/did/items/:nftId`, () => {
    return HttpResponse.json({
      nftId: '0xdid_ckb_id',
      name: 'did:ckb:alice',
      standard: 'did_ckb',
      ownerLockHash: '0xownerhash789',
      isLive: true,
      createdAtBlock: 100000,
      expiredAt: null,
    });
  }),

  // --- Network peer crawler mock handlers ---

  http.get(`${API_BASE}/network/summary`, () => {
    return HttpResponse.json({
      enabled: false,
      hasData: false,
      lastRound: null,
      activeRound: null,
    });
  }),

  http.get(`${API_BASE}/network/distributions`, () => {
    return HttpResponse.json({
      verifiedRetained: 0,
      sameNetworkReachable: 0,
      verifiedUnavailable: 0,
      versions: [],
      countries: [],
      asns: [],
      protocols: [],
    });
  }),

  http.get(`${API_BASE}/network/history`, ({ request }) => {
    const metric = new URL(request.url).searchParams.get('metric') ?? 'verifiedPeers';
    // Canned daily points that already exclude the current (incomplete) day.
    const days = [1751328000, 1751414400, 1751500800];

    if (metric === 'versionShare') {
      return HttpResponse.json({
        metric,
        granularity: 'day',
        points: days.map((ts) => ({
          ts,
          scalar: 0,
          buckets: [
            { label: '0.114.0', count: 4 },
            { label: '0.113.0', count: 2 },
          ],
        })),
      });
    }
    if (metric === 'countryShare') {
      return HttpResponse.json({
        metric,
        granularity: 'day',
        points: days.map((ts) => ({
          ts,
          scalar: 0,
          buckets: [
            { label: 'United States', count: 3 },
            { label: 'Germany', count: 2 },
          ],
        })),
      });
    }
    // Scalar metrics: verifiedPeers / reachablePeers.
    const base = metric === 'reachablePeers' ? 60 : 100;
    return HttpResponse.json({
      metric,
      granularity: 'day',
      points: days.map((ts, i) => ({ ts, scalar: base + i * 10, buckets: [] })),
    });
  }),

  http.get(`${API_BASE}/network/peers`, ({ request }) => {
    // One currently reachable verified peer and one advertised candidate that has never completed
    // a same-network Identify. Honour the exact display-state filter.
    const peers = [
      {
        peerId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
        crawlerDialState: 'reachable',
        participation: {
          discoveryAdvertised: false,
          directSessionObserved: false,
          crawlerIdentified: true,
        },
        sessionInitiators: [],
        primaryAddr: '/ip4/1.2.3.4/tcp/8115',
        version: '0.114.0',
        country: 'United States',
        asn: 'AS24940 Hetzner',
        lastAdvertisedAt: 1751500800,
        lastDialObservedAt: 1751500800,
        latestPositiveObservedAt: 1751500800,
        lastReachableAt: 1751500800,
        rttMs: 42,
      },
      {
        peerId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
        crawlerDialState: 'advertisedUnverified',
        participation: {
          discoveryAdvertised: true,
          directSessionObserved: false,
          crawlerIdentified: false,
        },
        sessionInitiators: [],
        primaryAddr: '/ip4/5.6.7.8/tcp/8115',
        version: null,
        country: null,
        asn: null,
        lastAdvertisedAt: 1751414400,
        lastDialObservedAt: 1751414400,
        latestPositiveObservedAt: 1751414400,
        lastReachableAt: null,
        rttMs: null,
      },
      {
        peerId: 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
        crawlerDialState: 'noCompletedObservation',
        participation: {
          discoveryAdvertised: false,
          directSessionObserved: true,
          crawlerIdentified: false,
        },
        sessionInitiators: ['peerInitiated'],
        primaryAddr: null,
        version: null,
        country: null,
        asn: null,
        lastAdvertisedAt: null,
        lastDialObservedAt: null,
        latestPositiveObservedAt: 1751500900,
        lastReachableAt: null,
        rttMs: null,
      },
    ];
    const searchParams = new URL(request.url).searchParams;
    const state = searchParams.get('state');
    const observation = searchParams.get('observation');
    const items = peers.filter((peer) => {
      const peerObservation =
        peer.crawlerDialState === 'reachable'
          ? 'sameNetworkIdentified'
          : peer.crawlerDialState === 'advertisedUnverified'
            ? 'noAuthenticatedSessionBeforeDeadline'
            : null;
      return (
        (!state || peer.crawlerDialState === state) &&
        (!observation || peerObservation === observation)
      );
    });
    return HttpResponse.json({ items, nextCursor: null });
  }),

  http.get(`${API_BASE}/network/peers/:peerId`, ({ params }) => {
    const peerId = String(params.peerId);
    const directOnly = peerId.startsWith('c');
    return HttpResponse.json({
      peerId,
      observationVantage: 'configuredLocalCkbRpcObserverAndThisCrawler',
      crawlerDialState: peerId.startsWith('a')
        ? 'reachable'
        : directOnly
          ? 'noCompletedObservation'
          : 'advertisedUnverified',
      participation: {
        discoveryAdvertised: !peerId.startsWith('a') && !directOnly,
        directSessionObserved: directOnly,
        crawlerIdentified: peerId.startsWith('a'),
      },
      sessionInitiators: directOnly ? ['peerInitiated'] : [],
      firstDiscoveredAt: directOnly ? null : 1751328000,
      lastAdvertisedAt: directOnly ? null : 1751500800,
      latestPositiveObservedAt: directOnly ? 1751500900 : 1751500800,
      aliases: directOnly
        ? []
        : [
            {
              address: peerId.startsWith('a') ? '/ip4/1.2.3.4/tcp/8115' : '/ip4/5.6.7.8/tcp/8115',
              firstAdvertisedAt: 1751328000,
              lastAdvertisedAt: 1751500800,
              lastVerifiedAt: peerId.startsWith('a') ? 1751500800 : null,
            },
          ],
      lastCompleted: directOnly
        ? null
        : {
            roundId: 5,
            outcome: peerId.startsWith('a') ? 'sameNetworkIdentified' : 'exhausted',
            observations: [
              {
                address: peerId.startsWith('a') ? '/ip4/1.2.3.4/tcp/8115' : '/ip4/5.6.7.8/tcp/8115',
                roundId: 5,
                observedAt: 1751500800,
                elapsedMs: peerId.startsWith('a') ? 42 : 10000,
                result: peerId.startsWith('a')
                  ? 'sameNetworkIdentified'
                  : 'noAuthenticatedSessionBeforeDeadline',
              },
            ],
            consecutiveExhaustedRounds: peerId.startsWith('a') ? 0 : 2,
          },
      active: null,
      verified: peerId.startsWith('a')
        ? {
            ownAddrs: ['/ip4/1.2.3.4/tcp/8115'],
            clientVersion: '0.114.0',
            flags: 1,
            protocols: ['/ckb/2'],
            firstSeen: 1751328000,
            lastSeen: 1751500800,
            lastReachableAt: 1751500800,
            country: 'United States',
            asn: 'AS24940 Hetzner',
            rttMs: 42,
            discovery: {
              validNodesMessages: 1,
              validResponseMessages: 1,
              validAnnounceMessages: 0,
              malformedMessages: 0,
              unexpectedMessages: 0,
              normalizedAdvertisedAddresses: 2,
              rejectedAdvertisedAddresses: 0,
            },
          }
        : null,
      advertisers:
        peerId.startsWith('a') || directOnly
          ? []
          : [
              {
                advertiserPeerId: 'aaaaaaaa'.repeat(8),
                alias: '/ip4/8.8.8.8/tcp/8114/p2p/peer-c',
                firstObservedAt: 1751328000,
                lastObservedAt: 1751414400,
                firstObservedRound: 4,
                lastObservedRound: 5,
                observationCount: 2,
              },
            ],
      directSessions: directOnly
        ? [
            {
              observerPeerId: 'dddddddd'.repeat(8),
              initiator: 'peerInitiated',
              firstObservedAt: 1751500900,
              lastObservedAt: 1751500900,
              firstObservedRound: 5,
              lastObservedRound: 5,
              observationCount: 1,
              clientVersion: '0.119.0',
              sessionAddresses: [],
              connectedDurationMs: 12000,
              lastPingDurationMs: null,
              protocols: [],
            },
          ]
        : [],
    });
  }),
];
