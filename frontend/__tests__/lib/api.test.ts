import { api } from '@/lib/api';
import { DOTBIT_COLLECTION_ID } from '@/lib/nft-collections';
import { server } from '../msw/server';
import { http, HttpResponse } from 'msw';

describe('api', () => {
  describe('query parameter building', () => {
    beforeEach(() => {
      vi.spyOn(global, 'fetch');
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it('builds query params for getBlocks with cursor', async () => {
      server.use(
        http.get('*/api/v1/blocks', ({ request }) => {
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
        http.get('*/api/v1/transactions', ({ request }) => {
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
        http.get('*/api/v1/cells/live', ({ request }) => {
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

    it('builds query params for getTokens with search', async () => {
      server.use(
        http.get('*/api/v1/tokens', ({ request }) => {
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
        http.get('*/api/v1/dao/deposits', ({ request }) => {
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
        http.get('*/api/v1/dao/summary/0xabc123', () => {
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
        http.get('*/api/v1/dao/calculator', ({ request }) => {
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
        http.get('*/api/v1/scripts', ({ request }) => {
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

    it('builds query params for getAssets with sorting', async () => {
      server.use(
        http.get('*/api/v1/assets', ({ request }) => {
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
        http.get('*/api/v1/assets', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('type')).toBe('nft');
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

      await api.getAssets({ type: 'nft', standard: 'spore' });
    });

    it('fetches script occupation chart by script name', async () => {
      server.use(
        http.get('*/api/v1/scripts/:name/charts/occupation', ({ params }) => {
          expect(params.name).toBe('SECP256K1_BLAKE160');
          return HttpResponse.json({
            title: 'SECP256K1_BLAKE160 Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getScriptOccupationChart('SECP256K1_BLAKE160');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('builds date range query params for script occupation chart by name', async () => {
      server.use(
        http.get('*/api/v1/scripts/:name/charts/occupation', ({ request, params }) => {
          expect(params.name).toBe('SECP256K1_BLAKE160');
          const url = new URL(request.url);
          expect(url.searchParams.get('from')).toBe('2024-01-01');
          expect(url.searchParams.get('to')).toBe('2024-01-31');
          return HttpResponse.json({
            title: 'SECP256K1_BLAKE160 Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getScriptOccupationChart('SECP256K1_BLAKE160', {
        from: '2024-01-01',
        to: '2024-01-31',
      });
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('builds query params for script occupation chart by code hash', async () => {
      server.use(
        http.get('*/api/v1/scripts/charts/occupation', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('code_hash')).toBe('0x1234');
          expect(url.searchParams.get('script_kind')).toBe('type');
          return HttpResponse.json({
            title: '0x1234 Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getScriptOccupationChartByCodeHash('0x1234', 'type');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('fetches token occupation chart by type hash', async () => {
      server.use(
        http.get('*/api/v1/tokens/:typeHash/charts/occupation', ({ params }) => {
          expect(params.typeHash).toBe('0x1234');
          return HttpResponse.json({
            title: 'TEST Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getTokenOccupationChart('0x1234');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('builds date range query params for token occupation chart', async () => {
      server.use(
        http.get('*/api/v1/tokens/:typeHash/charts/occupation', ({ request, params }) => {
          expect(params.typeHash).toBe('0x1234');
          const url = new URL(request.url);
          expect(url.searchParams.get('from')).toBe('2024-01-10');
          expect(url.searchParams.get('to')).toBe('2024-01-20');
          return HttpResponse.json({
            title: 'TEST Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getTokenOccupationChart('0x1234', {
        from: '2024-01-10',
        to: '2024-01-20',
      });
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('fetches spore cluster occupation chart', async () => {
      server.use(
        http.get('*/api/v1/spore/clusters/:clusterId/charts/occupation', ({ params }) => {
          expect(params.clusterId).toBe('0xabcd');
          return HttpResponse.json({
            title: 'My Cluster Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getSporeClusterOccupationChart('0xabcd');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('fetches spore nft occupation chart', async () => {
      server.use(
        http.get('*/api/v1/spore/nfts/:sporeId/charts/occupation', ({ params }) => {
          expect(params.sporeId).toBe('0x9999');
          return HttpResponse.json({
            title: 'Spore Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getSporeNftOccupationChart('0x9999');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('fetches spore dob decoded result', async () => {
      server.use(
        http.get('*/api/v1/spore/nfts/:sporeId/decode', ({ params }) => {
          expect(params.sporeId).toBe('0x9999');
          return HttpResponse.json({
            sporeId: '0x9999',
            contentType: 'dob/0',
            dnaHex: '0a01ff00',
            traits: [{ name: 'Background', value: 'red' }],
            svgMarkup: null,
            issues: [],
          });
        })
      );

      const decoded = await api.getSporeNftDecoded('0x9999');
      expect(decoded.contentType).toBe('dob/0');
      expect(decoded.traits[0].name).toBe('Background');
    });

    it('fetches nft collection detail', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/:collectionId', ({ params }) => {
          expect(params.collectionId).toBe('0xcollection');
          return HttpResponse.json({
            collectionId: '0xcollection',
            standard: 'm-nft',
            name: 'Test Collection',
            totalCount: 10,
            liveCount: 7,
            liveCapacity: '1000',
            liveOccupiedCapacity: '600',
          });
        })
      );

      const collection = await api.getNftCollection('0xcollection');
      expect(collection.collectionId).toBe('0xcollection');
      expect(collection.liveOccupiedCapacity).toBe('600');
    });

    it('normalizes dotbit alias for nft collection detail requests', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/:collectionId', ({ params }) => {
          expect(params.collectionId).toBe(DOTBIT_COLLECTION_ID);
          return HttpResponse.json({
            collectionId: DOTBIT_COLLECTION_ID,
            standard: 'dotbit',
            name: '.bit',
            totalCount: 10,
            liveCount: 7,
            liveCapacity: '1000',
            liveOccupiedCapacity: '600',
          });
        })
      );

      const collection = await api.getNftCollection('.bit');
      expect(collection.standard).toBe('dotbit');
    });

    it('fetches nft collection occupation chart', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/:collectionId/charts/occupation', ({ params }) => {
          expect(params.collectionId).toBe('0xcollection');
          return HttpResponse.json({
            title: 'Test Collection Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getNftCollectionOccupationChart('0xcollection');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('normalizes dotbit alias for nft collection requests', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/:collectionId/charts/occupation', ({ params }) => {
          expect(params.collectionId).toBe(DOTBIT_COLLECTION_ID);
          return HttpResponse.json({
            title: '.bit Capacity Occupation',
            data: [],
            series: [],
          });
        })
      );

      const chart = await api.getNftCollectionOccupationChart('dotbit');
      expect(chart.title).toContain('Capacity Occupation');
    });

    it('builds query params for nft collection items search', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/:collectionId/items', ({ request, params }) => {
          const url = new URL(request.url);
          expect(params.collectionId).toBe(DOTBIT_COLLECTION_ID);
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
        })
      );

      const result = await api.getNftCollectionItems('.bit', {
        limit: 20,
        cursor: 'abc',
        search: 'alice',
        status: 'live',
      });
      expect(result.hasMore).toBe(false);
    });

    it('fetches dotbit item detail', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/dotbit/items/:nftId', ({ params }) => {
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
        http.get('*/api/v1/assets/nfts/dotbit/items/:nftId/activities', ({ request, params }) => {
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
        })
      );

      const activities = await api.getDotbitItemActivities('0xabc', {
        limit: 20,
        cursor: '300:0',
        action: 'transfer',
      });
      expect(activities.data).toHaveLength(1);
      expect(activities.data[0].actions[0]).toBe('transfer');
    });

    it('fetches mnft item detail', async () => {
      server.use(
        http.get('*/api/v1/assets/nfts/items/:nftId', ({ params }) => {
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
        http.get('*/api/v1/assets/nfts/items/:nftId/activities', ({ request, params }) => {
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
        })
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
        http.get('*/api/v1/addresses/:addr/tokens', ({ request }) => {
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
        http.get('*/api/v1/blocks/999999', () => {
          return new HttpResponse(null, { status: 404 });
        })
      );

      await expect(api.getBlock('999999')).rejects.toThrow('API error: 404');
    });

    it('throws error for server errors', async () => {
      server.use(
        http.get('*/api/v1/statistics/network', () => {
          return new HttpResponse(null, { status: 500 });
        })
      );

      await expect(api.getNetworkStats()).rejects.toThrow('API error: 500');
    });

    it('includes backend error message when present', async () => {
      server.use(
        http.get('*/api/v1/statistics/network', () => {
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
        http.get('*/api/v1/blocks/12345', () => {
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
        http.get('*/api/v1/transactions/0x123/detail', () => {
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
        http.get('*/api/v1/search', ({ request }) => {
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
        http.post('*/api/v1/scripts/lookup', async ({ request }) => {
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
        http.get('*/api/v1/scripts/code-cell', ({ request }) => {
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
        http.get('*/api/v1/scripts/code-cell', () => {
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
        http.post('*/api/v1/transactions/0x123/calculate-cycles', () => {
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
        http.get('*/api/v1/graph/cell/0xabc/0', ({ request }) => {
          const url = new URL(request.url);
          expect(url.searchParams.get('depth')).toBe('3');
          return HttpResponse.json({ nodes: [], links: [] });
        })
      );

      await api.getCellGraph('0xabc', 0, 3);
    });

    it('getTransactionGraph with default depth', async () => {
      server.use(
        http.get('*/api/v1/graph/transaction/0x123', ({ request }) => {
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
        http.get('*/api/v1/forks', ({ request }) => {
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
        http.get('*/api/v1/forks/1', () => {
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
        http.get('*/api/v1/forks/recent', () => {
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
