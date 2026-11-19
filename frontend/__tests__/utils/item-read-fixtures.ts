import {
  api,
  type ItemKind,
  type ItemProtocol,
  type CollectionItem,
  type DotCellItem,
  type SporeDobDecoded,
  type CursorPaginatedResponse,
  type RankedCursorPage,
  type AssetCapacityHistory,
} from '@/lib/api';
import * as producer from '@/__tests__/msw/fixtures/item-producer';
import {
  collectionActivityFeedProducer,
  collectionTransactionsProducer,
} from '@/__tests__/msw/fixtures/collection-feed';
import { pagedRead } from '@/__tests__/utils/paged-read';
export const emptyPage = <T>(data: T[] = []): CursorPaginatedResponse<T> =>
  pagedRead({
    data,
    total: data.length,
    limit: 50,
    hasMore: false,
    nextCursor: null,
  });
/** One page of a ranked list (a holders list), its first row the first (lane exactness, part B5). */
export const rankedPage = <T>(data: T[] = []): RankedCursorPage<T> => ({
  ...emptyPage(data),
  firstRank: 1,
});
const SECP_CODE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
export const protocols: Record<ItemKind, ItemProtocol> = {
  spore: producer.sporeProtocolProducer,
  mnft: producer.mnftProtocolProducer,
  dotbit: producer.dotbitV4ProtocolProducer,
  'bit-cell': producer.bitCellProtocolProducer,
  did: producer.didProtocolProducer,
  dotcell: producer.dotcellProtocolProducer,
};
export function identityItem(
  kind: Exclude<ItemKind, 'spore' | 'mnft' | 'dotcell'>
): CollectionItem {
  const p = protocols[kind],
    v = p.versions.at(-1)!;
  return {
    nftId: p.itemId,
    name: kind === 'did' ? 'did:ckb:ceirceirceirceirceirceirceirceir' : 'alice.bit',
    standard: kind === 'did' ? 'did_ckb' : kind === 'bit-cell' ? 'bit_cell' : 'dotbit',
    ownerLockHash: v.cell.ownerLockHash,
    ownerAddress: v.cell.ownerAddress,
    isLive: v.cell.isLive,
    createdAtBlock: v.cell.blockNumber,
    txHash: v.cell.txHash,
    outputIndex: v.cell.outputIndex,
    expiredAt: kind === 'did' ? null : 1900000000,
  };
}
export function dotcellItem(): DotCellItem {
  const p = producer.dotcellProtocolProducer,
    v = p.versions[0],
    d = v.decoded;
  return {
    identityId: p.itemId,
    label: d.label,
    name: d.name,
    isLive: true,
    createdAtBlock: v.cell.blockNumber,
    createdAtTx: v.cell.txHash,
    layoutVersion: d.layoutVersion,
    namespaceArgs: '0x' + '34'.repeat(20),
    expiredAt: Number(d.expiredAt),
    state: 'active',
    graceEndsAt: 1902592000,
    owner: d.owner,
    manager: d.manager,
    // Round 3 (contracts R-A · R-5): the owner holds this one live name.
    ownerNameCount: 1,
    sale: null,
    records: d.records,
    recordsHash: d.recordsHash,
    nextId: d.nextId,
    parent: null,
    children: [],
    childrenHasMore: false,
    childrenNextCursor: null,
    liveOutPoint: { txHash: v.cell.txHash, index: v.cell.outputIndex },
  };
}
export function mockItemReads() {
  vi.restoreAllMocks();
  vi.spyOn(api, 'getItemCellVersions').mockImplementation(async (kind) => ({
    data: protocols[kind].versions.map((v) => v.cell),
    total: protocols[kind].versions.length,
  }));
  vi.spyOn(api, 'getItemProtocol').mockImplementation(async (kind) =>
    structuredClone(protocols[kind])
  );
  vi.spyOn(api, 'getItemActivityFeed').mockResolvedValue(producer.itemActivityFeedProducer);
  vi.spyOn(api, 'getItemTransactions').mockResolvedValue(producer.itemTransactionsProducer);
  vi.spyOn(api, 'getSporeObject').mockResolvedValue(producer.sporeDetailProducer);
  vi.spyOn(api, 'getSporeCluster').mockResolvedValue(producer.clusterDetailProducer);
  vi.spyOn(api, 'getSporeClusterProtocol').mockResolvedValue({
    kind: 'spore_cluster',
    clusterId: producer.clusterDetailProducer.clusterId,
    source: 'canonicalLiveTypeIndex',
    current: null,
  });
  vi.spyOn(api, 'getObjectGenetics').mockResolvedValue(producer.objectGeneticsProducer);
  const decoded: SporeDobDecoded = {
    status: 'decoded',
    sporeId: producer.sporeDetailProducer.sporeId,
    contentType: 'dob/0',
    dnaHex: producer.objectGeneticsProducer.dnaHex,
    traits: producer.objectGeneticsProducer.cachedVmTraits.traits,
    media: [],
    render: null,
    issues: [],
  };
  vi.spyOn(api, 'getSporeObjectDecoded').mockResolvedValue(decoded);
  vi.spyOn(api, 'getObjectCollection').mockResolvedValue(producer.objectCollectionDetailProducer);
  vi.spyOn(api, 'getCollectionInsights').mockResolvedValue(producer.collectionInsightsProducer);
  vi.spyOn(api, 'getCollectionGallery').mockResolvedValue(
    producer.collectionGalleryFilteredProducer
  );
  vi.spyOn(api, 'getCollectionActivityFeed').mockResolvedValue(collectionActivityFeedProducer);
  vi.spyOn(api, 'getCollectionTransactions').mockResolvedValue(collectionTransactionsProducer);
  const chart: AssetCapacityHistory = {
    source: 'indexedChainTimeUtc8',
    completeThrough: '2023-11-14',
    anchor: { blockNumber: 100, blockHash: '0x' + 'aa'.repeat(32), chainTime: '1700000000000' },
    title: 'Collection capacity',
    data: [
      { date: '2023-11-13', values: { occupiedCapacity: '0', freeCapacity: '0', capacity: '0' } },
      {
        date: '2023-11-14',
        values: {
          occupiedCapacity: '183200000000',
          freeCapacity: '216800000000',
          capacity: '400000000000',
        },
      },
    ],
    series: [
      { key: 'occupiedCapacity', label: 'Occupied', color: '#f59e0b' },
      { key: 'freeCapacity', label: 'Free', color: '#00c389' },
    ],
  };
  vi.spyOn(api, 'getSporeClusterCapacityChart').mockResolvedValue(chart);
  vi.spyOn(api, 'getObjectCollectionCapacityChart').mockResolvedValue(chart);
  vi.spyOn(api, 'getIdentityCollectionCapacityChart').mockResolvedValue(chart);
  // B4 second fix round: a holder whose address, lock hash and lock are one lock's, a gallery
  // row's default-lock owner (the cell's owner beside the default lock's name was not one)
  const holder = producer.collectionGalleryProducer.data[0];
  const holders = rankedPage([
    {
      lockScriptHash: holder.ownerLockHash,
      address: holder.ownerAddress,
      itemCount: 6,
      lockCodeHash: holder.lockCodeHash,
      lockKind: holder.lockKind,
    },
    // Round 3 (contracts R-A · R-2): an unindexed lock has neither address nor code hash.
    {
      lockScriptHash: '0x' + '50'.repeat(32),
      address: null,
      itemCount: 2,
      lockCodeHash: null,
      lockKind: null,
    },
  ]);
  vi.spyOn(api, 'getSporeClusterHolders').mockResolvedValue(holders);
  vi.spyOn(api, 'getObjectCollectionHolders').mockResolvedValue(holders);
  vi.spyOn(api, 'getIdentityCollectionHolders').mockResolvedValue(holders);
  vi.spyOn(api, 'getObjectCollectionItems').mockResolvedValue(emptyPage());
  vi.spyOn(api, 'getIdentityCollectionItems').mockResolvedValue(
    emptyPage([identityItem('dotbit')])
  );
  vi.spyOn(api, 'getIdentityCollection').mockResolvedValue({
    collectionId: 'dotbit',
    standard: 'dotbit',
    name: '.bit',
    totalCount: 2,
    liveCount: 1,
    holdersCount: 1,
    activitiesCount: 3,
    ownedCapacity: '100000000000',
    ownedKnowledge: '20400000000',
  });
  vi.spyOn(api, 'getDidPdsDistribution').mockResolvedValue(producer.didPdsProducer);
  // Round 3 (contracts L3): no live .bit Cell names this account.
  vi.spyOn(api, 'getDotbitItemDetail').mockResolvedValue({
    ...identityItem('dotbit'),
    bitCells: [],
  });
  vi.spyOn(api, 'getDidCkbItemDetail').mockResolvedValue(identityItem('did'));
  vi.spyOn(api, 'getBitCellItemDetail').mockResolvedValue(identityItem('bit-cell'));
  vi.spyOn(api, 'getDotCellItemDetail').mockResolvedValue(dotcellItem());
  vi.spyOn(api, 'getDotCellRing').mockResolvedValue({
    namespaceArgs: '0x' + '34'.repeat(20),
    rootOutPoint: { txHash: '0x' + '99'.repeat(32), index: 0 },
    firstId: producer.dotcellProtocolProducer.itemId,
    liveCount: 1,
  });
  vi.spyOn(api, 'getDotCellItemChildren').mockResolvedValue(emptyPage());
}
