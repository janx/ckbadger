import { api, resolveApiBase } from '@/lib/api';
const DOTBIT_COLLECTION_ID = '0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f';
const DID_CKB_COLLECTION_ID = '0x6469645f636b625f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f';
import { DEFAULT_API_BASE } from '@/lib/runtime-config';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';

describe('api', () => {
  describe('resolveApiBase', () => {
    it('uses runtime config apiBase when present', () => {
      const base = resolveApiBase({
        apiBase: 'http://127.0.0.1:9101/api/v1/',
      });

      expect(base).toBe('http://127.0.0.1:9101/api/v1');
    });

    it('falls back to default when runtime config apiBase is blank', () => {
      expect(resolveApiBase({ apiBase: '   ' })).toBe(DEFAULT_API_BASE);
    });

    it('falls back to default when runtime config is missing', () => {
      expect(resolveApiBase({})).toBe(DEFAULT_API_BASE);
    });
  });

  describe('query parameter building', () => {
    beforeEach(() => {
      vi.spyOn(global, 'fetch');
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it('builds query params for getBlocks with cursor', async () => {
      server.use(
        http.get('/api/:network/v1/blocks', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('cursor')).toBe('test-cursor');
          expect(url.searchParams.get('limit')).toBe('25');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 25,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getBlocks({ cursor: 'test-cursor', limit: 25 });
    });

    it('builds query params for getTransactions with blockNumber', async () => {
      server.use(
        http.get('/api/:network/v1/transactions', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('block_number')).toBe('12345');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getTransactions({ blockNumber: 12345 });
    });

    it('builds query params for getLiveCells with filters', async () => {
      server.use(
        http.get('/api/:network/v1/cells/live', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('lock_script_hash')).toBe('0xabc123');
          expect(url.searchParams.get('type_script_hash')).toBe('0xdef456');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getLiveCells({ lockScriptHash: '0xabc123', typeScriptHash: '0xdef456' });
    });

    it('getLiveCellSummary reads the fixed-size summary endpoint', async () => {
      server.use(
        http.get('/api/:network/v1/cells/live-summary', () => {
          return HttpResponse.json({
            tip: { block: 123, hash: `0x${'ab'.repeat(32)}` },
            liveCells: 6,
            classes: { dao: 1, typedNonDao: 2, plain: 3 },
            dataBearing: 4,
          });
        })
      );

      const summary = await api.getLiveCellSummary();

      expect(summary.tip.block).toBe(123);
      expect(summary.liveCells).toBe(6);
      expect(summary.classes).toEqual({ dao: 1, typedNonDao: 2, plain: 3 });
      expect(summary.dataBearing).toBe(4);
    });

    it('builds query params for getTokens with search', async () => {
      server.use(
        http.get('/api/:network/v1/tokens', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('search')).toBe('USDT');
          expect(url.searchParams.get('standard')).toBe('sudt');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getTokens({ search: 'USDT', standard: 'sudt' });
    });

    it('builds query params for getDaoDeposits with status', async () => {
      server.use(
        http.get('/api/:network/v1/dao/deposits', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('status')).toBe('1');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getDaoDeposits({ status: 1 });
    });

    it('getAddressDaoSummary fetches DAO summary for address', async () => {
      server.use(
        http.get('/api/:network/v1/dao/summary/0xabc123', () => {
          return HttpResponse.json({
            hasDaoActivity: true,
            activeDepositsCount: 3,
            pendingWithdrawalsCount: 1,
            completedWithdrawalsCount: 5,
            totalLockedCapacity: '500000000000',
            totalLockedCkb: '5000',
            unclaimedCompensation: '12500000000',
            unclaimedCompensationCkb: '125',
            totalCompensationEarned: '25000000000',
            totalCompensationEarnedCkb: '250',
            estimatedApc: '4.86',
          });
        })
      );

      const result = await api.getAddressDaoSummary('0xabc123');
      expect(result.hasDaoActivity).toBe(true);
      expect(result.activeDepositsCount).toBe(3);
      expect(result.totalLockedCkb).toBe('5000');
      expect(result.estimatedApc).toBe('4.86');
    });

    it('builds query params for calculateDaoCompensation', async () => {
      server.use(
        http.get('/api/:network/v1/dao/calculator', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('capacity')).toBe('10000000000');
          expect(url.searchParams.get('deposit_block')).toBe('1000');
          expect(url.searchParams.get('withdraw_block')).toBe('2000');
          return HttpResponse.json({
            capacity: '10000000000',
            capacityCkb: '100',
            depositBlock: 1000,
            withdrawBlock: 2000,
            estimatedCompensation: '100000000',
            estimatedCompensationCkb: '1',
            totalWithdrawable: '10100000000',
            totalWithdrawableCkb: '101',
            apc: '3.5',
          });
        })
      );

      await api.calculateDaoCompensation('10000000000', 1000, 2000);
    });

    it('builds query params for getScripts with filters', async () => {
      server.use(
        http.get('/api/:network/v1/scripts', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('network')).toBe('mainnet');
          expect(url.searchParams.get('decoder_type')).toBe('lock');
          expect(url.searchParams.get('search')).toBe('secp256k1');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getScripts({ network: 'mainnet', decoderType: 'lock', search: 'secp256k1' });
    });

    it('maps script family list responses into known script rows', async () => {
      server.use(
        http.get('/api/:network/v1/scripts', () => {
          return HttpResponse.json({
            data: [
              {
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
              },
            ],
            total: 1,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      const result = await api.getScripts();
      expect(result.data).toEqual([
        expect.objectContaining({
          codeHash: 'family:default-lock',
          name: 'Default Lock',
          scriptKind: 'lock',
          ownedCapacitySum: '1500',
          ownedKnowledgeSum: '900',
          liveCellsCount: 10,
          cellsCount: 14,
        }),
      ]);
    });

    it('flattens script family detail into version deployment entries and preserves observed refs', async () => {
      server.use(
        http.get('/api/:network/v1/scripts/:name', ({ params }) => {
          expect(params.name).toBe('Default Lock');
          return HttpResponse.json({
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
                codeCellsTotal: 2,
                deployments: [
                  {
                    hashType: 'type',
                    typeReferenceHash:
                      '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
                    dataReferenceHash:
                      '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
                    codeCellTxHash:
                      '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    codeCellOutputIndex: 0,
                    deployedAt: 1700000000000,
                  },
                  {
                    hashType: 'data',
                    typeReferenceHash:
                      '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
                    dataReferenceHash:
                      '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
                    codeCellTxHash:
                      '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                    codeCellOutputIndex: 1,
                    deployedAt: 1700001000000,
                  },
                ],
                references: [
                  {
                    referenceHash:
                      '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
                    hashType: 'type',
                    liveCellsCount: 4,
                    cellsCount: 6,
                    ownedCapacitySum: '700',
                    ownedKnowledgeSum: '400',
                  },
                  {
                    referenceHash:
                      '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
                    hashType: 'data1',
                    liveCellsCount: 6,
                    cellsCount: 8,
                    ownedCapacitySum: '800',
                    ownedKnowledgeSum: '500',
                  },
                ],
              },
            ],
          });
        })
      );

      const result = await api.getScript('Default Lock');
      expect(result).toEqual([
        expect.objectContaining({
          codeHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
          hashType: 'type',
          typeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          dataHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
          codeCellTxHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          codeCellOutputIndex: 0,
          liveCellsCount: 10,
          cellsCount: 14,
          observedReferences: [
            expect.objectContaining({
              referenceHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
              hashType: 'type',
            }),
            expect.objectContaining({
              referenceHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
              hashType: 'data1',
            }),
          ],
        }),
        expect.objectContaining({
          codeHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
          hashType: 'data',
          typeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          dataHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
          codeCellTxHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          codeCellOutputIndex: 1,
          liveCellsCount: 10,
          cellsCount: 14,
        }),
      ]);
    });

    it('builds query params for getAssets with sorting', async () => {
      server.use(
        http.get('/api/:network/v1/assets', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('type')).toBe('token');
          expect(url.searchParams.get('sort_key')).toBe('transfers_24h');
          expect(url.searchParams.get('sort_direction')).toBe('asc');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getAssets({ type: 'token', sortKey: 'transfers24h', sortDirection: 'asc' });
    });

    it('builds query params for getAssets with standard filter', async () => {
      server.use(
        http.get('/api/:network/v1/assets', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('type')).toBe('object');
          expect(url.searchParams.get('standard')).toBe('spore');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      await api.getAssets({ type: 'object', standard: 'spore' });
    });

    it('fetches script capacity chart by script name', async () => {
      server.use(
        http.get('/api/:network/v1/scripts/:name/charts/capacity-history', ({ params }) => {
          expect(params.name).toBe('SECP256K1_BLAKE160');
          return HttpResponse.json({
            title: 'SECP256K1_BLAKE160 Capacity History',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getScriptCapacityChart('SECP256K1_BLAKE160');
      expect(chart.title).toContain('Capacity History');
    });

    it('normalizes fiber stats from the backend response shape', async () => {
      server.use(
        http.get('/api/:network/v1/fiber/stats', () => {
          return HttpResponse.json({
            totalChannels: 42,
            openChannels: 10,
            totalCapacityLocked: '500000000000',
          });
        })
      );

      const stats = await api.getFiberStats();

      expect(stats).toEqual({
        totalChannels: 42,
        openChannels: 10,
        closedChannels: 32,
        totalCapacityLocked: '500000000000',
      });
    });

    it('builds date range query params for script capacity chart by name', async () => {
      server.use(
        http.get(
          '/api/:network/v1/scripts/:name/charts/capacity-history',
          ({ request, params }) => {
            expect(params.name).toBe('SECP256K1_BLAKE160');
            const url = new URL(request.url);
            expect(url.searchParams.get('from')).toBe('2024-01-01');
            expect(url.searchParams.get('to')).toBe('2024-01-31');
            return HttpResponse.json({
              title: 'SECP256K1_BLAKE160 Capacity History',
              data: [],
              series: [],
            });
          }
        )
      );

      const chart = await api.getScriptCapacityChart('SECP256K1_BLAKE160', {
        from: '2024-01-01',
        to: '2024-01-31',
      });
      expect(chart.title).toContain('Capacity History');
    });

    it('builds query params for script capacity chart by code hash', async () => {
      server.use(
        http.get('/api/:network/v1/scripts/charts/capacity-history', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('code_hash')).toBe('0x1234');
          expect(url.searchParams.get('script_kind')).toBe('type');
          return HttpResponse.json({
            title: '0x1234 Capacity History',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getScriptCapacityChartByCodeHash('0x1234', 'type');
      expect(chart.title).toContain('Capacity History');
    });

    it('fetches token capacity chart by type hash', async () => {
      server.use(
        http.get('/api/:network/v1/tokens/:typeHash/charts/capacity-history', ({ params }) => {
          expect(params.typeHash).toBe('0x1234');
          return HttpResponse.json({
            title: 'TEST Capacity History',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getTokenCapacityChart('0x1234');
      expect(chart.title).toContain('Capacity History');
    });

    it('builds date range query params for token capacity chart', async () => {
      server.use(
        http.get(
          '/api/:network/v1/tokens/:typeHash/charts/capacity-history',
          ({ request, params }) => {
            expect(params.typeHash).toBe('0x1234');
            const url = new URL(request.url);
            expect(url.searchParams.get('from')).toBe('2024-01-10');
            expect(url.searchParams.get('to')).toBe('2024-01-20');
            return HttpResponse.json({
              title: 'TEST Capacity History',
              data: [],
              series: [],
            });
          }
        )
      );

      const chart = await api.getTokenCapacityChart('0x1234', {
        from: '2024-01-10',
        to: '2024-01-20',
      });
      expect(chart.title).toContain('Capacity History');
    });

    it('fetches spore cluster capacity chart', async () => {
      server.use(
        http.get(
          '/api/:network/v1/spore/clusters/:clusterId/charts/capacity-history',
          ({ params }) => {
            expect(params.clusterId).toBe('0xabcd');
            return HttpResponse.json({
              title: 'My Cluster Capacity History',
              data: [],
              series: [],
            });
          }
        )
      );

      const chart = await api.getSporeClusterCapacityChart('0xabcd');
      expect(chart.title).toContain('Capacity History');
    });

    it('fetches spore object capacity chart', async () => {
      server.use(
        http.get(
          '/api/:network/v1/spore/objects/:sporeId/charts/capacity-history',
          ({ params }) => {
            expect(params.sporeId).toBe('0x9999');
            return HttpResponse.json({
              title: 'Spore Capacity History',
              data: [],
              series: [],
            });
          }
        )
      );

      const chart = await api.getSporeObjectCapacityChart('0x9999');
      expect(chart.title).toContain('Capacity History');
    });

    it('fetches spore dob decoded result', async () => {
      server.use(
        http.get('/api/:network/v1/spore/objects/:sporeId/decode', ({ params }) => {
          expect(params.sporeId).toBe('0x9999');
          return HttpResponse.json({
            status: 'ok',
            sporeId: '0x9999',
            contentType: 'dob/0',
            dnaHex: '0a01ff00',
            traits: [{ name: 'Background', value: 'red' }],
            media: [],
            issues: [],
          });
        })
      );

      const decoded = await api.getSporeObjectDecoded('0x9999');
      expect(decoded.contentType).toBe('dob/0');
      expect(decoded.traits[0].name).toBe('Background');
    });

    it('fetches object collection detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/objects/:collectionId', ({ params }) => {
          expect(params.collectionId).toBe('0xcollection');
          return HttpResponse.json({
            collectionId: '0xcollection',
            standard: 'm-nft',
            name: 'Test Collection',
            totalCount: 10,
            liveCount: 7,
            holdersCount: 5,
            activitiesCount: 20,
            ownedCapacity: '1000',
            ownedKnowledge: '600',
          });
        })
      );

      const collection = await api.getObjectCollection('0xcollection');
      expect(collection.collectionId).toBe('0xcollection');
      expect(collection.ownedKnowledge).toBe('600');
    });

    it('fetches dotbit identity collection detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/identities/:collectionId', ({ params }) => {
          expect(params.collectionId).toBe('dotbit');
          return HttpResponse.json({
            collectionId: DOTBIT_COLLECTION_ID,
            standard: 'dotbit',
            name: '.bit',
            totalCount: 10,
            liveCount: 7,
            holdersCount: 5,
            activitiesCount: 20,
          });
        })
      );

      const collection = await api.getIdentityCollection('dotbit');
      expect(collection.standard).toBe('dotbit');
    });

    it('fetches did:ckb identity collection detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/identities/:collectionId', ({ params }) => {
          expect(params.collectionId).toBe('did:ckb');
          return HttpResponse.json({
            collectionId: DID_CKB_COLLECTION_ID,
            standard: 'did_ckb',
            name: 'did:ckb',
            totalCount: 10,
            liveCount: 7,
            holdersCount: 5,
            activitiesCount: 20,
          });
        })
      );

      const collection = await api.getIdentityCollection('did:ckb');
      expect(collection.standard).toBe('did_ckb');
    });

    it('fetches object collection capacity chart', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/objects/:collectionId/charts/capacity-history',
          ({ params }) => {
            expect(params.collectionId).toBe('0xcollection');
            return HttpResponse.json({
              title: 'Test Collection Capacity History',
              data: [],
              series: [],
            });
          }
        )
      );

      const chart = await api.getObjectCollectionCapacityChart('0xcollection');
      expect(chart.title).toContain('Capacity History');
    });

    it('fetches object collection capacity chart by collection id', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/objects/:collectionId/charts/capacity-history',
          ({ params }) => {
            expect(params.collectionId).toBe('0xcollection2');
            return HttpResponse.json({
              title: 'Collection Capacity History',
              data: [],
              series: [],
            });
          }
        )
      );

      const chart = await api.getObjectCollectionCapacityChart('0xcollection2');
      expect(chart.title).toContain('Capacity History');
    });

    it('builds query params for identity collection items search', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/identities/:collectionId/items',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.collectionId).toBe('dotbit');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('abc');
            expect(url.searchParams.get('search')).toBe('alice');
            expect(url.searchParams.get('status')).toBe('live');
            return HttpResponse.json({
              data: [],
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const result = await api.getIdentityCollectionItems('dotbit', {
        limit: 20,
        cursor: 'abc',
        search: 'alice',
        status: 'live',
      });
      expect(result.hasMore).toBe(false);
    });

    it('builds query params for identity collection holders', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/identities/:collectionId/holders',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.collectionId).toBe('dotbit');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('2:abcd');
            return HttpResponse.json({
              data: [],
              total: 0,
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const result = await api.getIdentityCollectionHolders('dotbit', {
        limit: 20,
        cursor: '2:abcd',
      });
      expect(result.total).toBe(0);
    });

    it('builds query params for identity collection activities', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/identities/:collectionId/activities',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.collectionId).toBe('dotbit');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('300:1');
            expect(url.searchParams.get('action')).toBe('transfer');
            return HttpResponse.json({
              data: [],
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const result = await api.getIdentityCollectionActivities('dotbit', {
        limit: 20,
        cursor: '300:1',
        action: 'transfer',
      });
      expect(result.hasMore).toBe(false);
    });

    it('builds query params for spore cluster holders', async () => {
      server.use(
        http.get('/api/:network/v1/spore/clusters/:clusterId/holders', ({ request, params }) => {
          const url = new URL(request.url);
          expect(params.clusterId).toBe('0xcluster');
          expect(url.searchParams.get('limit')).toBe('20');
          expect(url.searchParams.get('cursor')).toBe('4:ffff');
          return HttpResponse.json({
            data: [],
            total: 0,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      const result = await api.getSporeClusterHolders('0xcluster', {
        limit: 20,
        cursor: '4:ffff',
      });
      expect(result.total).toBe(0);
    });

    it('builds query params for spore cluster activities', async () => {
      server.use(
        http.get('/api/:network/v1/spore/clusters/:clusterId/activities', ({ request, params }) => {
          const url = new URL(request.url);
          expect(params.clusterId).toBe('0xcluster');
          expect(url.searchParams.get('limit')).toBe('20');
          expect(url.searchParams.get('cursor')).toBe('300:0');
          expect(url.searchParams.get('action')).toBe('burn');
          return HttpResponse.json({
            data: [],
            limit: 20,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      const result = await api.getSporeClusterActivities('0xcluster', {
        limit: 20,
        cursor: '300:0',
        action: 'burn',
      });
      expect(result.hasMore).toBe(false);
    });

    it('fetches dotbit item detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/identities/dotbit/items/:nftId', ({ params }) => {
          expect(params.nftId).toBe('0xabc');
          return HttpResponse.json({
            nftId: '0xabc',
            name: 'alice.bit',
            standard: 'dotbit',
            ownerLockHash: '0xowner',
            isLive: false,
            createdAtBlock: 123,
            expiredAt: 1800000000,
            txHash: null,
            outputIndex: null,
          });
        })
      );

      const detail = await api.getDotbitItemDetail('0xabc');
      expect(detail.nftId).toBe('0xabc');
      expect(detail.name).toBe('alice.bit');
      expect(detail.isLive).toBe(false);
    });

    it('fetches dotbit item activities with query params', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/identities/dotbit/items/:nftId/activities',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.nftId).toBe('0xabc');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('300:0');
            expect(url.searchParams.get('action')).toBe('transfer');
            return HttpResponse.json({
              data: [
                {
                  txHash: '0xtx',
                  blockNumber: 300,
                  txIndex: 0,
                  timestamp: '1700000300',
                  actions: ['transfer'],
                },
              ],
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const activities = await api.getDotbitItemActivities('0xabc', {
        limit: 20,
        cursor: '300:0',
        action: 'transfer',
      });
      expect(activities.data).toHaveLength(1);
      expect(activities.data[0].actions[0]).toBe('transfer');
    });

    it('fetches did:ckb item detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/identities/did/items/:nftId', ({ params }) => {
          expect(params.nftId).toBe('0xdid');
          return HttpResponse.json({
            nftId: '0xdid',
            name: 'did:alice.ckb',
            standard: 'did_ckb',
            ownerLockHash: '0xowner',
            isLive: true,
            createdAtBlock: 321,
            txHash: null,
            outputIndex: null,
          });
        })
      );

      const detail = await api.getDidCkbItemDetail('0xdid');
      expect(detail.nftId).toBe('0xdid');
      expect(detail.name).toBe('did:alice.ckb');
      expect(detail.standard).toBe('did_ckb');
      expect(detail.isLive).toBe(true);
    });

    it('fetches did:ckb item activities with query params', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/identities/did/items/:nftId/activities',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.nftId).toBe('0xdid');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('300:0');
            expect(url.searchParams.get('action')).toBe('transfer');
            return HttpResponse.json({
              data: [
                {
                  txHash: '0xtx',
                  blockNumber: 300,
                  txIndex: 0,
                  timestamp: '1700000300',
                  actions: ['transfer'],
                },
              ],
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const activities = await api.getDidCkbItemActivities('0xdid', {
        limit: 20,
        cursor: '300:0',
        action: 'transfer',
      });
      expect(activities.data).toHaveLength(1);
      expect(activities.data[0].actions[0]).toBe('transfer');
    });

    it('fetches .bit Cell item detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/identities/bit-cell/items/:nftId', ({ params }) => {
          expect(params.nftId).toBe('0xbitcell');
          return HttpResponse.json({
            nftId: '0xbitcell',
            name: 'alice.bit-cell',
            standard: 'bit_cell',
            ownerLockHash: '0xowner',
            isLive: true,
            createdAtBlock: 456,
            expiredAt: 1800000000,
            txHash: '0xtx',
            outputIndex: 3,
          });
        })
      );

      const detail = await api.getBitCellItemDetail('0xbitcell');
      expect(detail.nftId).toBe('0xbitcell');
      expect(detail.standard).toBe('bit_cell');
      expect(detail.outputIndex).toBe(3);
    });

    it('fetches .bit Cell item activities with query params', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/identities/bit-cell/items/:nftId/activities',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.nftId).toBe('0xbitcell');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('456:0');
            expect(url.searchParams.get('action')).toBe('transfer');
            return HttpResponse.json({
              data: [],
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const activities = await api.getBitCellItemActivities('0xbitcell', {
        limit: 20,
        cursor: '456:0',
        action: 'transfer',
      });
      expect(activities.data).toEqual([]);
    });

    it('fetches mnft item detail', async () => {
      server.use(
        http.get('/api/:network/v1/assets/objects/items/:nftId', ({ params }) => {
          expect(params.nftId).toBe('0xmnfttoken');
          return HttpResponse.json({
            nftId: '0xmnfttoken',
            standard: 'm-nft',
            isLive: true,
            ownerLockHash: '0xowner',
            createdAtBlock: 123,
            tokenIndex: 99,
            characteristicHex: '0x0102030405060708',
            configure: 3,
            state: 1,
            txHash: '0xtx',
            outputIndex: 7,
            class: {
              classId: '0xclass',
              issuerId: '0xissuer',
              name: 'Class A',
              description: 'desc',
              renderer: 'renderer:v1',
              total: 100,
              issued: 99,
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
          });
        })
      );

      const detail = await api.getMnftItemDetail('0xmnfttoken');
      expect(detail.nftId).toBe('0xmnfttoken');
      expect(detail.class.classId).toBe('0xclass');
      expect(detail.tokenIndex).toBe(99);
    });

    it('fetches mnft item activities with query params', async () => {
      server.use(
        http.get(
          '/api/:network/v1/assets/objects/items/:nftId/activities',
          ({ request, params }) => {
            const url = new URL(request.url);
            expect(params.nftId).toBe('0xmnfttoken');
            expect(url.searchParams.get('limit')).toBe('20');
            expect(url.searchParams.get('cursor')).toBe('300:0');
            expect(url.searchParams.get('action')).toBe('transfer');
            return HttpResponse.json({
              data: [
                {
                  txHash: '0xtx',
                  blockNumber: 300,
                  txIndex: 0,
                  timestamp: '1700000300',
                  actions: ['transfer'],
                },
              ],
              limit: 20,
              hasMore: false,
              nextCursor: null,
            });
          }
        )
      );

      const activities = await api.getMnftItemActivities('0xmnfttoken', {
        limit: 20,
        cursor: '300:0',
        action: 'transfer',
      });
      expect(activities.data[0].actions[0]).toBe('transfer');
    });

    it('builds query params for getAddressTokens', async () => {
      server.use(
        http.get('/api/:network/v1/addresses/:addr/tokens', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('limit')).toBe('50');
          expect(url.searchParams.get('cursor')).toBe('test-cursor');
          return HttpResponse.json({
            data: [
              {
                typeScriptHash: '0x123',
                standard: 'sudt',
                name: 'Test Token',
                symbol: 'TEST',
                decimals: 8,
                iconUrl: null,
                balance: '1000000000000',
              },
            ],
            total: 1,
            limit: 50,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      const result = await api.getAddressTokens('0xabc', { limit: 50, cursor: 'test-cursor' });
      expect(result.data).toHaveLength(1);
      expect(result.data[0].symbol).toBe('TEST');
    });
  });

  describe('error handling', () => {
    it('throws error for non-ok responses', async () => {
      server.use(
        http.get('/api/:network/v1/blocks/999999', () => {
          return new HttpResponse(null, { status: 404 });
        })
      );

      await expect(api.getBlock('999999')).rejects.toThrow('API error: 404');
    });

    it('throws error for server errors', async () => {
      server.use(
        http.get('/api/:network/v1/statistics/network', () => {
          return new HttpResponse(null, { status: 500 });
        })
      );

      await expect(api.getNetworkStats()).rejects.toThrow('API error: 500');
    });

    it('includes backend error message when present', async () => {
      server.use(
        http.get('/api/:network/v1/statistics/network', () => {
          return HttpResponse.json(
            { error: 'internal_error', message: 'negative live capacity in list scripts' },
            { status: 500 }
          );
        })
      );

      await expect(api.getNetworkStats()).rejects.toThrow(
        'API error: 500 - negative live capacity in list scripts'
      );
    });

    it('preserves structured warmup_pending error details', async () => {
      server.use(
        http.get('/api/:network/v1/statistics/network', () => {
          return HttpResponse.json(
            { error: 'warmup_pending', message: 'script cache unavailable; warmup in progress' },
            { status: 503 }
          );
        })
      );

      await expect(api.getNetworkStats()).rejects.toMatchObject({
        code: 'warmup_pending',
        status: 503,
        apiMessage: 'script cache unavailable; warmup in progress',
      });
    });
  });

  describe('specific endpoints', () => {
    it('getBlock fetches by hash or number', async () => {
      const mockBlock = {
        number: 12345,
        hash: '0xabc',
        parentHash: '0xdef',
        timestamp: '2024-01-01T00:00:00Z',
        transactionsCount: 5,
      };

      server.use(
        http.get('/api/:network/v1/blocks/12345', () => {
          return HttpResponse.json(mockBlock);
        })
      );

      const result = await api.getBlock(12345);
      expect(result.number).toBe(12345);
    });

    it('getTransactionDetail fetches detailed tx info', async () => {
      const mockTxDetail = {
        hash: '0x123',
        blockNumber: 100,
        inputs: [],
        outputs: [],
        confirmations: 24,
      };

      server.use(
        http.get('/api/:network/v1/transactions/0x123/detail', () => {
          return HttpResponse.json(mockTxDetail);
        })
      );

      const result = await api.getTransactionDetail('0x123');
      expect(result.confirmations).toBe(24);
    });

    it('search returns results', async () => {
      const mockSearchResult = {
        results: [{ resultType: 'block', id: '12345', label: 'Block #12345', url: '/block/12345' }],
        query: '12345',
      };

      server.use(
        http.get('/api/:network/v1/search', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('q')).toBe('12345');
          return HttpResponse.json(mockSearchResult);
        })
      );

      const result = await api.search('12345');
      expect(result.results).toHaveLength(1);
      expect(result.results[0].resultType).toBe('block');
    });

    it('lookupScripts sends POST request', async () => {
      server.use(
        http.post('/api/:network/v1/scripts/lookup', async ({ request }) => {
          const body = (await request.json()) as { codeHashes: string[] };
          expect(body.codeHashes).toContain('0xabc');
          return HttpResponse.json({
            '0xabc': {
              codeHash: '0xabc',
              name: 'Test Script',
              scriptKind: 'lock',
              decoderType: null,
            },
          });
        })
      );

      const result = await api.lookupScripts(['0xabc']);
      expect(result['0xabc'].name).toBe('Test Script');
    });

    it('lookupScripts returns empty object for empty array', async () => {
      const result = await api.lookupScripts([]);
      expect(result).toEqual({});
    });

    it('getCodeCell sends request with code_hash and hash_type', async () => {
      server.use(
        http.get('/api/:network/v1/scripts/code-cell', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('code_hash')).toBe('0xabc123');
          expect(url.searchParams.get('hash_type')).toBe('type');
          return HttpResponse.json({
            txHash: '0xdef456',
            outputIndex: 0,
          });
        })
      );

      const result = await api.getCodeCell('0xabc123', 'type');
      expect(result.txHash).toBe('0xdef456');
      expect(result.outputIndex).toBe(0);
    });

    it('getCodeCell returns null values when not found', async () => {
      server.use(
        http.get('/api/:network/v1/scripts/code-cell', () => {
          return HttpResponse.json({
            txHash: null,
            outputIndex: null,
          });
        })
      );

      const result = await api.getCodeCell('0x000', 'data');
      expect(result.txHash).toBeNull();
      expect(result.outputIndex).toBeNull();
    });

    it('triggerCyclesCalculation sends POST request', async () => {
      server.use(
        http.post('/api/:network/v1/transactions/0x123/calculate-cycles', () => {
          return HttpResponse.json({ status: 'queued', cycles: null, error: null });
        })
      );

      const result = await api.triggerCyclesCalculation('0x123');
      expect(result.status).toBe('queued');
    });
  });

  describe('graph endpoints', () => {
    it('getCellGraph with custom depth', async () => {
      server.use(
        http.get('/api/:network/v1/graph/cell/0xabc/0', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('depth')).toBe('3');
          return HttpResponse.json({ nodes: [], links: [] });
        })
      );

      await api.getCellGraph('0xabc', 0, 3);
    });

    it('getTransactionGraph with default depth', async () => {
      server.use(
        http.get('/api/:network/v1/graph/transaction/0x123', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('depth')).toBe('2');
          return HttpResponse.json({ nodes: [], links: [] });
        })
      );

      await api.getTransactionGraph('0x123');
    });
  });

  describe('fork endpoints', () => {
    it('getForks returns paginated list', async () => {
      server.use(
        http.get('/api/:network/v1/forks', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('limit')).toBe('10');
          return HttpResponse.json({
            data: [
              {
                id: 1,
                eventType: 'auto',
                depth: 3,
                forkPointNumber: 1000,
                orphanedBlocksCount: 3,
                orphanedTransactionsCount: 15,
                detectedAt: '2024-01-15T10:30:00Z',
              },
            ],
            total: 1,
            limit: 10,
            hasMore: false,
            nextCursor: null,
          });
        })
      );

      const result = await api.getForks({ limit: 10 });
      expect(result.data).toHaveLength(1);
      expect(result.data[0].eventType).toBe('auto');
      expect(result.data[0].depth).toBe(3);
    });

    it('getForkDetail returns event with orphaned data', async () => {
      server.use(
        http.get('/api/:network/v1/forks/1', () => {
          return HttpResponse.json({
            event: {
              id: 1,
              eventType: 'auto',
              depth: 3,
            },
            orphanedBlocks: [{ blockNumber: 1001, transactionsCount: 5 }],
            orphanedTransactions: [{ txHash: '0xaaa', blockNumber: 1001 }],
          });
        })
      );

      const result = await api.getForkDetail(1);
      expect(result.event.id).toBe(1);
      expect(result.orphanedBlocks).toHaveLength(1);
      expect(result.orphanedTransactions).toHaveLength(1);
    });

    it('getRecentReorg returns deep fork status', async () => {
      server.use(
        http.get('/api/:network/v1/forks/recent', () => {
          return HttpResponse.json({
            hasRecentReorg: true,
            reorg: {
              id: 1,
              eventType: 'auto',
              depth: 3,
            },
            deepFork: {
              detected: true,
              depth: 50,
              dbTip: 1000,
              chainTip: 1050,
              forkPoint: 1000,
            },
          });
        })
      );

      const result = await api.getRecentReorg();
      expect(result.hasRecentReorg).toBe(true);
      expect(result.deepFork.detected).toBe(true);
      expect(result.deepFork.depth).toBe(50);
    });
  });
});
