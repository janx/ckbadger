import {
  api,
  type ItemKind,
  type ItemProtocol,
  type CollectionItem,
  type DotCellItem,
  type SporeDobDecoded,
  type CursorPaginatedResponse,
  type AssetCapacityHistory,
} from '@/lib/api';
import * as producer from '@/__tests__/msw/fixtures/item-producer';
import {
  collectionActivityFeedProducer,
  collectionTransactionsProducer,
} from '@/__tests__/msw/fixtures/collection-feed';
export const emptyPage = <T>(data: T[] = []): CursorPaginatedResponse<T> => ({
  data,
  total: data.length,
  limit: 50,
  hasMore: false,
  nextCursor: null,
});
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
      { date: '2023-11-13', values: { used: '0', unused: '0' } },
      { date: '2023-11-14', values: { used: '183200000000', unused: '216800000000' } },
    ],
    series: [
      { key: 'used', label: 'Used', color: '#f59e0b' },
      { key: 'unused', label: 'Unused', color: '#00c389' },
    ],
  };
  vi.spyOn(api, 'getSporeClusterCapacityChart').mockResolvedValue(chart);
  vi.spyOn(api, 'getObjectCollectionCapacityChart').mockResolvedValue(chart);
  vi.spyOn(api, 'getIdentityCollectionCapacityChart').mockResolvedValue(chart);
  const holder = producer.sporeCellVersionsProducer.data[0];
  const holders = emptyPage([
    { lockScriptHash: holder.ownerLockHash, address: holder.ownerAddress, itemCount: 6 },
    { lockScriptHash: '0x' + '50'.repeat(32), address: null, itemCount: 2 },
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
  vi.spyOn(api, 'getDotbitItemDetail').mockResolvedValue(identityItem('dotbit'));
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
