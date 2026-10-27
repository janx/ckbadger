import type {
  Address,
  AddressHoldings,
  AddressCell,
  AddressToken,
  DaoDeposit,
  Activity,
  AddressTransaction,
} from '@/lib/api';

export const addressHash = `0x${'3e'.repeat(32)}`;
export const addressText =
  'ckb1qrgqep8saj8agswr30pls73hra28ry8jlnlc3ejzh3dl2ju7xxpjxqgqq83503v79qrpczf2e6wdgf6jj8am56udwsgxzykc';
export const fixtureHash = (n: number) => `0x${n.toString(16).padStart(64, '0')}`;
export const addressFixture: Address = {
  lockScriptHash: addressHash,
  address: addressText,
  balance: '965117835338057',
  commonKnowledgeSize: '712700000000',
  liveCellsCount: 40,
  transactionsCount: 1021,
  pendingSummary: null,
  lockScript: {
    codeHash: fixtureHash(100),
    hashType: 'type',
    args: '0x0001e347c59e28061c092ace9cd4275291fbba6b8d74',
  },
  lockScriptInfo: {
    codeHash: fixtureHash(100),
    name: 'JoyID',
    scriptKind: 'lock',
    deprecated: false,
  },
};
export const emptyComposition = {
  pureCkb: 0,
  btcCkb: 0,
  decentralizedMixture: 0,
  centralizedMixture: 0,
  unknown: 0,
  decodePending: 0,
};
const collections = [
  ['拯救民主', 6, 'btc_ckb'],
  ['Nervape', 3, 'btc_ckb'],
  ['Nervape Mouth', 1, 'btc_ckb'],
  ['Unicorn', 1, 'pure_ckb'],
  ['SOUL Dragon Eggs', 1, 'centralized_mixture'],
  ['Lightning Explorer Badge', 1, 'centralized_mixture'],
  ['Lightning Pioneer Badge', 1, 'centralized_mixture'],
] as const;
export const holdingsFixture: AddressHoldings = {
  daoUnclaimedCompensation: '25987084255174',
  estimatedApc: '1.99',
  depositCellsCount: 5,
  withdrawalRequestCellsCount: 0,
  lockScriptHash: addressHash,
  balance: addressFixture.balance,
  occupiedCapacity: '712700000000',
  freeCapacity: '5457135338057',
  daoFreeCapacity: '958948000000000',
  liveCellsCount: 40,
  holdings: { dao: 5, token: 9, object: 14, identity: 1 },
  classes: {
    dao: { cellCount: 5, capacity: '959000000000000', occupiedCapacity: '52000000000' },
    token: { cellCount: 15, capacity: '2150760266154', occupiedCapacity: '214000000000' },
    object: { cellCount: 14, capacity: '409300000000', occupiedCapacity: '376100000000' },
    identity: { cellCount: 1, capacity: '22799993047', occupiedCapacity: '22700000000' },
    script: { cellCount: 3, capacity: '100000000000', occupiedCapacity: '35300000000' },
    plain: { cellCount: 2, capacity: '3434975078856', occupiedCapacity: '12600000000' },
  },
  objectComposition: { ...emptyComposition, pureCkb: 1, btcCkb: 10, centralizedMixture: 3 },
  objectCollections: collections.map(([name, itemCount, tier], i) => ({
    id: fixtureHash(200 + i),
    standard: 'spore',
    name,
    itemCount,
    compositionCounts: {
      ...emptyComposition,
      [tier === 'btc_ckb' ? 'btcCkb' : tier === 'pure_ckb' ? 'pureCkb' : 'centralizedMixture']:
        itemCount,
    },
  })),
};
const tokenValues = [
  ['iCKB', 'iCKB', '8465187699473', 8],
  ['Wrapped CKB', 'wCKB', '1947960266154', 8],
  ['LP-UTXOSwap V1', 'LP', '642547115111', 8],
  ['Seal', 'Seal', '391295560000', 8],
  ['STB', 'STB', '174181101054', 8],
  ['USDI', 'USDI', '309464541', 6],
  ['LP-UTXOSwap V1', 'LP', '24361725795', 8],
  ['RUSD', 'RUSD', '1000000000', 8],
  ['MEMES', 'MEMES', '1', 8],
] as const;
export const addressTokensFixture: AddressToken[] = tokenValues.map(
  ([name, symbol, balance, decimals], i) => ({
    typeScriptHash: fixtureHash(300 + i),
    standard: 'xudt',
    name,
    symbol,
    balance,
    decimals,
    iconUrl: null,
  })
);
export const addressDaoFixture: DaoDeposit[] = [5240000, 3500000, 600000, 200000, 50000].map(
  (v, i) => ({
    txHash: fixtureHash(400 + i),
    outputIndex: 0,
    lockScriptHash: addressHash,
    lockCodeHash: fixtureHash(100),
    capacity: (BigInt(v) * BigInt(100000000)).toString(),
    depositBlockNumber: 16381488 + i,
    depositTimestamp: '2025-06-06T00:00:00Z',
    status: 'deposited',
    withdrawRequestBlock: null,
    withdrawRequestTimestamp: null,
    withdrawRequestTxHash: null,
    withdrawRequestOutputIndex: null,
    withdrawBlock: null,
    withdrawTimestamp: null,
    withdrawTxHash: null,
    withdrawToOutputIndex: null,
    compensation: null,
  })
);
export const addressObjectsFixture: AddressCell[] = collections.flatMap(
  ([name, count, tier], collection) =>
    Array.from({ length: count }, (_, i) => ({
      txHash: fixtureHash(500 + collection * 10 + i),
      outputIndex: 0,
      capacity: collection === 6 ? '28400000000' : '29300000000',
      lockScriptHash: addressHash,
      typeScriptHash: null,
      typeCodeHash: null,
      typeHashType: null,
      dataSize: 100,
      createdAtBlock: 13495427,
      class: 'object' as const,
      dao: null,
      heldSinceBlock: null,
      occupiedCapacity: collection === 6 ? '26400000000' : '26900000000',
      expiresAt: null,
      display: {
        kind: 'object' as const,
        standard: 'spore',
        id: fixtureHash(600 + collection * 10 + i),
        name: null,
        collectionId: fixtureHash(200 + collection),
        collectionName: name,
        symbol: null,
        decimals: null,
        amount: null,
        compositionTier: tier,
      },
    }))
);
export const addressIdentitiesFixture: AddressCell[] = [
  {
    txHash: fixtureHash(700),
    outputIndex: 0,
    capacity: '22799993047',
    lockScriptHash: addressHash,
    typeScriptHash: null,
    typeCodeHash: null,
    typeHashType: null,
    dataSize: 100,
    createdAtBlock: 13495427,
    class: 'identity',
    dao: null,
    heldSinceBlock: 13495427,
    occupiedCapacity: '22700000000',
    expiresAt: 1802995200,
    display: {
      kind: 'identity',
      standard: 'bit_cell',
      id: '0x' + 'ab'.repeat(20),
      name: 'valar.bit',
      collectionId: null,
      collectionName: null,
      symbol: null,
      decimals: null,
      amount: null,
      compositionTier: null,
    },
  },
];
export const addressActivitiesFixture: Activity[] = [
  {
    txHash: fixtureHash(800),
    blockNumber: 18672923,
    txIndex: 1,
    timestamp: '1771632000000',
    ckbDelta: '437000000000',
    usedDelta: '6300000000',
    isCellbase: false,
    itemDeltas: [],
    typeCalls: [],
    lockCalls: [],
    protocolActions: [],
    participants: [],
    roles: [],
    tags: 0,
  },
];
export const addressTransactionsFixture: AddressTransaction[] = [
  {
    txHash: fixtureHash(800),
    blockNumber: 18672923,
    txType: 'received',
    capacityChange: '437000000000',
    timestamp: '1771632000000',
    inputsCount: 7,
    outputsCount: 11,
    fee: '12430',
    isCellbase: false,
    txSize: 5597,
    cycles: 7927025,
    cyclesStatus: 'complete',
    scriptLabels: ['Spore'],
  },
];
export const pageFixture = <T>(data: T[], total?: number) => ({
  data,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  ...(total === undefined ? {} : { total }),
});

export const addressDaoCellsFixture: AddressCell[] = addressDaoFixture.map((d) => ({
  txHash: d.txHash,
  outputIndex: d.outputIndex,
  capacity: d.capacity,
  occupiedCapacity: '10400000000',
  lockScriptHash: addressHash,
  typeScriptHash: null,
  typeCodeHash: null,
  typeHashType: null,
  dataSize: 8,
  createdAtBlock: d.depositBlockNumber,
  class: 'dao',
  display: null,
  expiresAt: null,
  heldSinceBlock: null,
  dao: d,
}));

function splitClassCapacity(
  kind: 'token' | 'script' | 'plain',
  index: number,
  count: number
): string {
  const capacity = BigInt(holdingsFixture.classes[kind].capacity);
  const part = capacity / BigInt(count);
  return (index === count - 1 ? capacity - part * BigInt(count - 1) : part).toString();
}
const tokenCells: AddressCell[] = Array.from({ length: 15 }, (_, i) => {
  const token = addressTokensFixture[i % 9];
  const occupied = i === 14 ? 152 : 142;
  // Six token types have two physical cells; these exact per-cell balances sum to each holding.
  const duplicate = i % 9 < 6;
  const half = BigInt(token.balance) / BigInt(2);
  const amount = duplicate ? (i < 9 ? half : BigInt(token.balance) - half) : BigInt(token.balance);
  return {
    txHash: fixtureHash(1000 + i),
    outputIndex: 0,
    capacity: splitClassCapacity('token', i, 15),
    occupiedCapacity: String(occupied * 100000000),
    lockScriptHash: addressHash,
    typeScriptHash: null,
    typeCodeHash: null,
    typeHashType: null,
    dataSize: 16,
    createdAtBlock: 18000000 + i,
    class: 'token',
    expiresAt: null,
    heldSinceBlock: null,
    dao: null,
    display: {
      kind: 'token',
      standard: token.standard,
      id: token.typeScriptHash,
      name: token.name,
      collectionId: null,
      collectionName: null,
      symbol: token.symbol,
      decimals: token.decimals,
      amount: amount.toString(),
      compositionTier: null,
    },
  };
});
const otherCells: AddressCell[] = (['script', 'plain'] as const).flatMap((kind) =>
  Array.from({ length: kind === 'script' ? 3 : 2 }, (_, i) => {
    const count = kind === 'script' ? 3 : 2,
      occupied = kind === 'plain' ? 63 : i === 2 ? 119 : 117;
    return {
      txHash: fixtureHash(1100 + (kind === 'plain' ? 10 : 0) + i),
      outputIndex: 0,
      capacity: splitClassCapacity(kind, i, count),
      occupiedCapacity: String(occupied * 100000000),
      lockScriptHash: addressHash,
      typeScriptHash: null,
      typeCodeHash: null,
      typeHashType: null,
      dataSize: kind === 'plain' ? 2 : 10,
      createdAtBlock: 18000000 + i,
      class: kind,
      expiresAt: null,
      heldSinceBlock: null,
      dao: null,
      display: null,
    };
  })
);
export const addressAllCellsFixture = [
  ...addressDaoCellsFixture,
  ...tokenCells,
  ...addressObjectsFixture,
  ...addressIdentitiesFixture,
  ...otherCells,
];
