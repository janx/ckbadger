// Captured unchanged from native CKB fixture + real API handlers; not live mainnet data.
import type {
  ObjectGenetics,
  CollectionInsights,
  CursorPaginatedResponse,
  GalleryObject,
  ItemCellVersions,
  DidPdsDistribution,
  ItemActivity,
  ItemTransaction,
  ItemProtocol,
  SporeDetail,
  SporeCluster,
  ObjectCollection,
  ClusterProtocol,
  AssetCapacityHistory,
} from '@/lib/api';
export const objectGeneticsProducer = {
  cachedVmTraits: {
    decodedAt: '1700000000',
    originalDecoderHashRecorded: false,
    source: 'cachedVm',
    status: 'decoded',
    traits: [
      {
        name: 'Face',
        value: 'Blue',
      },
    ],
  },
  currentPattern: {
    source: 'currentCanonicalDecoder',
    status: 'dob',
    steps: [
      {
        bytes: [
          {
            byteLength: 1,
            byteOffset: 0,
            declaredByteLength: '1',
            declaredByteOffset: '0',
            geneIndex: 0,
            rawHex: '01',
            truncated: false,
          },
        ],
        decoderBinaryHash: '0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c',
        decoderReference: {
          hash: '0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c',
          type: 'code_hash',
        },
        description: {
          format: 'jsonDob0',
          genes: [
            {
              byteLength: '1',
              byteOffset: '0',
              index: 0,
              name: 'Face',
              rule: 'options',
              ruleArguments: ['Red', 'Blue'],
              traitIndex: 0,
              typeTag: 'String',
              valueIndex: 0,
            },
          ],
          rangePolicy: 'clipToDna',
          status: 'supported',
        },
        step: 0,
      },
    ],
    version: 0,
  },
  dnaHex: '0x01',
  number: '18446744073709551614',
} satisfies ObjectGenetics;
export const collectionInsightsProducer = {
  calendarAnchor: null,
  calendarSource: 'indexedChainTimeUtc8',
  completeThrough: null,
  currentPattern: {
    source: 'currentCanonicalDecoder',
    status: 'dob',
    steps: [
      {
        bytes: [],
        decoderBinaryHash: '0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c',
        decoderReference: {
          hash: '0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c',
          type: 'code_hash',
        },
        description: {
          format: 'jsonDob0',
          genes: [
            {
              byteLength: '1',
              byteOffset: '0',
              index: 0,
              name: 'Face',
              rule: 'options',
              ruleArguments: ['Red', 'Blue'],
              traitIndex: 0,
              typeTag: 'String',
              valueIndex: 0,
            },
          ],
          rangePolicy: 'clipToDna',
          status: 'supported',
        },
        step: 0,
      },
    ],
    version: 0,
  },
  genetics: {
    complete: true,
    decodedItems: 8,
    eligibleItems: 8,
    failedItems: 0,
    liveItems: 8,
    nonDobItems: 0,
    pendingItems: 0,
    traits: [
      {
        name: 'Face',
        values: [
          {
            count: 4,
            value: 'Blue',
          },
          {
            count: 4,
            value: 'Red',
          },
        ],
      },
    ],
  },
  holderDistribution: [
    {
      holders: 0,
      items: 0,
      range: '1',
    },
    {
      holders: 1,
      items: 2,
      range: '2',
    },
    {
      holders: 0,
      items: 0,
      range: '3\u20135',
    },
    {
      holders: 1,
      items: 6,
      range: '6+',
    },
  ],
  weeklyActivity: [],
} satisfies CollectionInsights;
/** `/assets/objects/{class id}/insights` for an m-NFT class: real holder buckets, no DOB items. */
export const mnftCollectionInsightsProducer = {
  calendarAnchor: null,
  calendarSource: 'indexedChainTimeUtc8',
  completeThrough: null,
  currentPattern: { status: 'notDob', steps: [] },
  genetics: {
    complete: true,
    decodedItems: 0,
    eligibleItems: 0,
    failedItems: 0,
    liveItems: 8,
    nonDobItems: 8,
    pendingItems: 0,
    traits: [],
  },
  holderDistribution: [
    { holders: 0, items: 0, range: '1' },
    { holders: 1, items: 2, range: '2' },
    { holders: 0, items: 0, range: '3\u20135' },
    { holders: 1, items: 6, range: '6+' },
  ],
  weeklyActivity: [],
} satisfies CollectionInsights;
export const collectionInsightsPendingProducer = {
  calendarAnchor: null,
  calendarSource: 'indexedChainTimeUtc8',
  completeThrough: null,
  currentPattern: {
    source: 'currentCanonicalDecoder',
    status: 'dob',
    steps: [
      {
        bytes: [],
        decoderBinaryHash: '0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c',
        decoderReference: {
          hash: '0x13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c',
          type: 'code_hash',
        },
        description: {
          format: 'jsonDob0',
          genes: [
            {
              byteLength: '1',
              byteOffset: '0',
              index: 0,
              name: 'Face',
              rule: 'options',
              ruleArguments: ['Red', 'Blue'],
              traitIndex: 0,
              typeTag: 'String',
              valueIndex: 0,
            },
          ],
          rangePolicy: 'clipToDna',
          status: 'supported',
        },
        step: 0,
      },
    ],
    version: 0,
  },
  genetics: {
    complete: false,
    decodedItems: 7,
    eligibleItems: 8,
    failedItems: 0,
    liveItems: 8,
    nonDobItems: 0,
    pendingItems: 1,
    traits: [],
  },
  holderDistribution: [
    {
      holders: 0,
      items: 0,
      range: '1',
    },
    {
      holders: 1,
      items: 2,
      range: '2',
    },
    {
      holders: 0,
      items: 0,
      range: '3\u20135',
    },
    {
      holders: 1,
      items: 6,
      range: '6+',
    },
  ],
  weeklyActivity: [],
} satisfies CollectionInsights;
export const collectionGalleryFilteredProducer = {
  data: [
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
      decodeStatus: 'decoded',
      dnaHex: '0x01',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551614',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0202020202020202020202020202020202020202020202020202020202020202',
      traits: [
        {
          name: 'Face',
          value: 'Blue',
        },
      ],
      txHash: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
    },
  ],
  hasMore: true,
  limit: 1,
  nextCursor: '100:0x0202020202020202020202020202020202020202020202020202020202020202',
  total: 4,
} satisfies CursorPaginatedResponse<GalleryObject>;
export const didCellVersionsProducer = {
  data: [
    {
      anatomy: {
        capacityBytes: 8,
        dataBytes: 0,
        lockBytes: 53,
        typeBytes: 53,
      },
      blockNumber: 100,
      capacity: '30700000000',
      consumedAtBlock: 100,
      consumedByTx: '0x5252525252525252525252525252525252525252525252525252525252525252',
      dataHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
      isLive: false,
      lock: {
        args: '0x0101010101010101010101010101010101010101',
        codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
        hashType: 'type',
      },
      lockName: null,
      occupiedCapacity: '11400000000',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      timestamp: '1700000000100',
      txHash: '0x5151515151515151515151515151515151515151515151515151515151515151',
      txIndex: 1,
      type: {
        args: '0x4444444444444444444444444444444444444444',
        codeHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
        hashType: 'type',
      },
      typeName: null,
    },
    {
      anatomy: {
        capacityBytes: 8,
        dataBytes: 0,
        lockBytes: 53,
        typeBytes: 53,
      },
      blockNumber: 100,
      capacity: '32000000000',
      consumedAtBlock: null,
      consumedByTx: null,
      dataHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
      isLive: true,
      lock: {
        args: '0x0202020202020202020202020202020202020202',
        codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
        hashType: 'type',
      },
      lockName: null,
      occupiedCapacity: '11400000000',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgzqgpqyqszqgpqyqszqgpqyqszqgpqyqsqdnfwk',
      ownerLockHash: '0xb3acb1c3ac9785b18ea34b9cdc66dbe1327950280dd4055aee95ac1f40add994',
      timestamp: '1700000000100',
      txHash: '0x5252525252525252525252525252525252525252525252525252525252525252',
      txIndex: 2,
      type: {
        args: '0x4444444444444444444444444444444444444444',
        codeHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
        hashType: 'type',
      },
      typeName: null,
    },
  ],
  total: 2,
} satisfies ItemCellVersions;
export const didPdsProducer = {
  groups: [
    {
      count: 1,
      endpoint: 'https://pds.example',
    },
  ],
  source: 'currentCanonicalDocuments',
  totalLiveIdentities: 1,
  withoutPds: 0,
} satisfies DidPdsDistribution;
export const itemActivityFeedProducer = {
  data: [
    {
      blockNumber: 100,
      isCellbase: false,
      itemContext: {
        action: 'transfer',
        consumedCell: {
          anatomy: {
            capacityBytes: 8,
            dataBytes: 0,
            lockBytes: 53,
            typeBytes: 53,
          },
          blockNumber: 100,
          capacity: '30700000000',
          consumedAtBlock: 100,
          consumedByTx: '0x5252525252525252525252525252525252525252525252525252525252525252',
          dataHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
          isLive: false,
          lock: {
            args: '0x0101010101010101010101010101010101010101',
            codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
            hashType: 'type',
          },
          lockName: null,
          occupiedCapacity: '11400000000',
          outputIndex: 0,
          ownerAddress:
            'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
          ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
          timestamp: '1700000000100',
          txHash: '0x5151515151515151515151515151515151515151515151515151515151515151',
          txIndex: 1,
          type: {
            args: '0x4444444444444444444444444444444444444444',
            codeHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
            hashType: 'type',
          },
          typeName: null,
        },
        createdCell: {
          anatomy: {
            capacityBytes: 8,
            dataBytes: 0,
            lockBytes: 53,
            typeBytes: 53,
          },
          blockNumber: 100,
          capacity: '32000000000',
          consumedAtBlock: null,
          consumedByTx: null,
          dataHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
          isLive: true,
          lock: {
            args: '0x0202020202020202020202020202020202020202',
            codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
            hashType: 'type',
          },
          lockName: null,
          occupiedCapacity: '11400000000',
          outputIndex: 0,
          ownerAddress:
            'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgzqgpqyqszqgpqyqszqgpqyqszqgpqyqsqdnfwk',
          ownerLockHash: '0xb3acb1c3ac9785b18ea34b9cdc66dbe1327950280dd4055aee95ac1f40add994',
          timestamp: '1700000000100',
          txHash: '0x5252525252525252525252525252525252525252525252525252525252525252',
          txIndex: 2,
          type: {
            args: '0x4444444444444444444444444444444444444444',
            codeHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
            hashType: 'type',
          },
          typeName: null,
        },
        itemId: '0x4444444444444444444444444444444444444444',
      },
      lockCalls: [],
      participants: [
        {
          address: null,
          ckbDelta: '100',
          itemDeltas: [],
          lockHash: '0xb3acb1c3ac9785b18ea34b9cdc66dbe1327950280dd4055aee95ac1f40add994',
          lockHashPrefix: null,
          lockKind: null,
          roles: [],
          tags: 0,
          usedDelta: '0',
        },
      ],
      protocolActions: [],
      timestamp: '1700000100',
      txHash: '0x5252525252525252525252525252525252525252525252525252525252525252',
      txIndex: 2,
      typeCalls: [],
    },
  ],
  hasMore: true,
  limit: 1,
  nextCursor: '100:2',
  total: 2,
} satisfies CursorPaginatedResponse<ItemActivity>;
export const itemTransactionsProducer = {
  data: [
    {
      activity: {
        blockNumber: 100,
        isCellbase: false,
        lockCalls: [],
        participants: [
          {
            address: null,
            ckbDelta: '100',
            itemDeltas: [],
            lockHash: '0xb3acb1c3ac9785b18ea34b9cdc66dbe1327950280dd4055aee95ac1f40add994',
            lockHashPrefix: null,
            lockKind: null,
            roles: [],
            tags: 0,
            usedDelta: '0',
          },
        ],
        protocolActions: [],
        timestamp: '1700000100',
        txHash: '0x5151515151515151515151515151515151515151515151515151515151515151',
        txIndex: 1,
        typeCalls: [],
      },
      blockHash: '0x6464646464646464646464646464646464646464646464646464646464646464',
      blockNumber: 100,
      cycles: null,
      cyclesStatus: 'pending',
      fee: '0',
      hash: '0x5151515151515151515151515151515151515151515151515151515151515151',
      index: 1,
      inputsCount: 1,
      isCellbase: false,
      itemContext: {
        action: 'mint',
        consumedCell: null,
        createdCell: {
          anatomy: {
            capacityBytes: 8,
            dataBytes: 0,
            lockBytes: 53,
            typeBytes: 53,
          },
          blockNumber: 100,
          capacity: '30700000000',
          consumedAtBlock: 100,
          consumedByTx: '0x5252525252525252525252525252525252525252525252525252525252525252',
          dataHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
          isLive: false,
          lock: {
            args: '0x0101010101010101010101010101010101010101',
            codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
            hashType: 'type',
          },
          lockName: null,
          occupiedCapacity: '11400000000',
          outputIndex: 0,
          ownerAddress:
            'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
          ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
          timestamp: '1700000000100',
          txHash: '0x5151515151515151515151515151515151515151515151515151515151515151',
          txIndex: 1,
          type: {
            args: '0x4444444444444444444444444444444444444444',
            codeHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
            hashType: 'type',
          },
          typeName: null,
        },
        itemId: '0x4444444444444444444444444444444444444444',
      },
      outputCapacity: '30700000000',
      outputsCount: 1,
      timestamp: '2023-11-14T22:13:20.100+00:00',
      txSize: 204,
      value: '100',
    },
  ],
  hasMore: false,
  limit: 1,
  nextCursor: null,
  total: 2,
} satisfies CursorPaginatedResponse<ItemTransaction>;
export const sporeDetailProducer = {
  clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
  contentSize: 38,
  contentType: 'dob/0',
  createdAtBlock: 100,
  createdAtTx: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
  isLive: true,
  mediaProfile: {
    issues: [],
    sources: [],
    tier: 'unknown',
  },
  outputIndex: 0,
  ownedCapacity: '50000000000',
  ownedKnowledge: '22900000000',
  ownerAddress:
    'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
  ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
  sporeId: '0x0202020202020202020202020202020202020202020202020202020202020202',
  txHash: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
  // The Nervape 3x spore's type script hash, as /spore/objects/{id} reports it.
  typeScriptHash: '0x303e7b5e16db979e24e79690a8f3a3436cffd00e8c60b40e067db097136cc998',
} satisfies SporeDetail;
export const sporeCellVersionsProducer = {
  data: [
    {
      anatomy: {
        capacityBytes: 8,
        dataBytes: 103,
        lockBytes: 53,
        typeBytes: 65,
      },
      blockNumber: 100,
      capacity: '50000000000',
      consumedAtBlock: null,
      consumedByTx: null,
      dataHash: '0xc9d057d5666780ce964fba028388f3e4318803591c90327e7be9fc4d5568d002',
      isLive: true,
      lock: {
        args: '0x0101010101010101010101010101010101010101',
        codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
        hashType: 'type',
      },
      lockName: null,
      occupiedCapacity: '22900000000',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      timestamp: '1700000000100',
      txHash: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
      txIndex: 1,
      type: {
        args: '0x0202020202020202020202020202020202020202020202020202020202020202',
        codeHash: '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
        hashType: 'type',
      },
      typeName: null,
    },
  ],
  total: 1,
} satisfies ItemCellVersions;
export const clusterDetailProducer = {
  activitiesCount: 0,
  clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
  composition: {
    centralizedMixtureCount: 0,
    decentralizedMixtureCount: 0,
    onchainCount: 0,
    onchainRatio: '0.0000',
    pureCkbCount: 0,
    tier: 'unknown',
    unknownCount: 8,
  },
  createdAtBlock: 99,
  description:
    '{"dob":{"decoder":{"hash":"13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c","type":"code_hash"},"pattern":[["Face","String",0,1,"options",["Red","Blue"]]],"ver":0}}',
  holdersCount: 2,
  name: 'Genes',
  ownedCapacity: '400000000000',
  ownedKnowledge: '183200000000',
  ownerAddress:
    'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
  ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
  sporesCount: 8,
  totalCount: 8,
} satisfies SporeCluster;
export const objectCollectionDetailProducer = {
  activitiesCount: 0,
  collectionId: '0x7777777777777777777777777777777777777777777777777777777777777777',
  composition: {
    centralizedMixtureCount: 0,
    decentralizedMixtureCount: 0,
    onchainCount: 0,
    onchainRatio: '0.0000',
    pureCkbCount: 0,
    tier: 'unknown',
    unknownCount: 8,
  },
  holdersCount: 2,
  liveCount: 8,
  name: null,
  ownedCapacity: '400000000000',
  ownedKnowledge: '183200000000',
  standard: 'spore',
  totalCount: 8,
} satisfies ObjectCollection;
export const clusterProtocolProducer = {
  clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
  current: {
    cell: {
      anatomy: {
        capacityBytes: 8,
        dataBytes: 210,
        lockBytes: 53,
        typeBytes: 65,
      },
      blockNumber: 99,
      capacity: '100000000000',
      consumedAtBlock: null,
      consumedByTx: null,
      dataHash: '0xfb1d94058486657cdaa924e39e328bfff5a4376083914548b412fc7bed29dc23',
      isLive: true,
      lock: {
        args: '0x0101010101010101010101010101010101010101',
        codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
        hashType: 'type',
      },
      lockName: null,
      occupiedCapacity: '33600000000',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      timestamp: '1700000000099',
      txHash: '0xa5c53484e7aa75ca3d36da3b03aa560e7341f616e53b5ef1e03f03c3541f3261',
      txIndex: 0,
      type: {
        args: '0x7777777777777777777777777777777777777777777777777777777777777777',
        codeHash: '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075',
        hashType: 'type',
      },
      typeName: null,
    },
    dataHex:
      '0xd20000001000000019000000d20000000500000047656e6573b50000007b22646f62223a7b226465636f646572223a7b2268617368223a2231336361633738616438343832323032663138663964663465613730373631316333356639393433373566613033616537393132313331326464613939323563222c2274797065223a22636f64655f68617368227d2c227061747465726e223a5b5b2246616365222c22537472696e67222c302c312c226f7074696f6e73222c5b22526564222c22426c7565225d5d5d2c22766572223a307d7d',
    decoded: {
      description:
        '{"dob":{"decoder":{"hash":"13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c","type":"code_hash"},"pattern":[["Face","String",0,1,"options",["Red","Blue"]]],"ver":0}}',
      name: 'Genes',
    },
    fields: [
      {
        end: 25,
        name: 'name',
        start: 16,
        valueHex: '0x0500000047656e6573',
      },
      {
        end: 210,
        name: 'description',
        start: 25,
        valueHex:
          '0xb50000007b22646f62223a7b226465636f646572223a7b2268617368223a2231336361633738616438343832323032663138663964663465613730373631316333356639393433373566613033616537393132313331326464613939323563222c2274797065223a22636f64655f68617368227d2c227061747465726e223a5b5b2246616365222c22537472696e67222c302c312c226f7074696f6e73222c5b22526564222c22426c7565225d5d5d2c22766572223a307d7d',
      },
      {
        end: 210,
        name: 'mutantId',
        start: 210,
        valueHex: '0x',
      },
    ],
  },
  kind: 'spore_cluster',
  source: 'canonicalLiveTypeIndex',
} satisfies ClusterProtocol;
export const collectionGalleryProducer = {
  data: [
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x18b224b944d7ac51e1f9ebf48ab3f24938a04b29022bd56e2923cae7203e2f6c',
      decodeStatus: 'decoded',
      dnaHex: '0x00',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551615',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0101010101010101010101010101010101010101010101010101010101010101',
      traits: [
        {
          name: 'Face',
          value: 'Red',
        },
      ],
      txHash: '0x18b224b944d7ac51e1f9ebf48ab3f24938a04b29022bd56e2923cae7203e2f6c',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
      decodeStatus: 'decoded',
      dnaHex: '0x01',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551614',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0202020202020202020202020202020202020202020202020202020202020202',
      traits: [
        {
          name: 'Face',
          value: 'Blue',
        },
      ],
      txHash: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0xc356cacc09685ce9c8e4e8c33717fdba09549a567bc01a5b8330d5196256173e',
      decodeStatus: 'decoded',
      dnaHex: '0x02',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551613',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0303030303030303030303030303030303030303030303030303030303030303',
      traits: [
        {
          name: 'Face',
          value: 'Red',
        },
      ],
      txHash: '0xc356cacc09685ce9c8e4e8c33717fdba09549a567bc01a5b8330d5196256173e',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x5bfd5e48d14c052786424184333cc8b8a98a3e31cc2f54b4c4fad5f7274f7513',
      decodeStatus: 'decoded',
      dnaHex: '0x03',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551612',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0404040404040404040404040404040404040404040404040404040404040404',
      traits: [
        {
          name: 'Face',
          value: 'Blue',
        },
      ],
      txHash: '0x5bfd5e48d14c052786424184333cc8b8a98a3e31cc2f54b4c4fad5f7274f7513',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x7a8a972290ecef95113de6b7e242161fcec3e8a8ea4200dc282b124c1b0faf0e',
      decodeStatus: 'decoded',
      dnaHex: '0x04',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551611',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0505050505050505050505050505050505050505050505050505050505050505',
      traits: [
        {
          name: 'Face',
          value: 'Red',
        },
      ],
      txHash: '0x7a8a972290ecef95113de6b7e242161fcec3e8a8ea4200dc282b124c1b0faf0e',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0xb9b7d51b3a4b152fe791b986a181951bd9f8767a2f3c895f5fe3db9fe926209a',
      decodeStatus: 'decoded',
      dnaHex: '0x05',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551610',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
      ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
      sporeId: '0x0606060606060606060606060606060606060606060606060606060606060606',
      traits: [
        {
          name: 'Face',
          value: 'Blue',
        },
      ],
      txHash: '0xb9b7d51b3a4b152fe791b986a181951bd9f8767a2f3c895f5fe3db9fe926209a',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x01394b4bff7caff7c6c06d6323223846d4257b2d45bccd4f2c4f252defac4c73',
      decodeStatus: 'decoded',
      dnaHex: '0x06',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551609',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgzqgpqyqszqgpqyqszqgpqyqszqgpqyqsqdnfwk',
      ownerLockHash: '0xb3acb1c3ac9785b18ea34b9cdc66dbe1327950280dd4055aee95ac1f40add994',
      sporeId: '0x0707070707070707070707070707070707070707070707070707070707070707',
      traits: [
        {
          name: 'Face',
          value: 'Red',
        },
      ],
      txHash: '0x01394b4bff7caff7c6c06d6323223846d4257b2d45bccd4f2c4f252defac4c73',
    },
    {
      clusterId: '0x7777777777777777777777777777777777777777777777777777777777777777',
      contentSize: 38,
      contentType: 'dob/0',
      createdAtBlock: 100,
      createdAtTx: '0x85d942f1ba6b75a85a4e6ba02e3b7a717636d8213389e1de52f5287f84bded50',
      decodeStatus: 'decoded',
      dnaHex: '0x07',
      isLive: true,
      mediaProfile: {
        issues: [],
        sources: [],
        tier: 'unknown',
      },
      number: '18446744073709551608',
      outputIndex: 0,
      ownerAddress:
        'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgzqgpqyqszqgpqyqszqgpqyqszqgpqyqsqdnfwk',
      ownerLockHash: '0xb3acb1c3ac9785b18ea34b9cdc66dbe1327950280dd4055aee95ac1f40add994',
      sporeId: '0x0808080808080808080808080808080808080808080808080808080808080808',
      traits: [
        {
          name: 'Face',
          value: 'Blue',
        },
      ],
      txHash: '0x85d942f1ba6b75a85a4e6ba02e3b7a717636d8213389e1de52f5287f84bded50',
    },
  ],
  hasMore: false,
  limit: 100,
  nextCursor: null,
  total: 8,
} satisfies CursorPaginatedResponse<GalleryObject>;
export const clusterCapacityHistoryProducer = {
  anchor: {
    blockHash: '0x0202020202020202020202020202020202020202020202020202020202020202',
    blockNumber: 2,
    chainTime: '1793059200000',
  },
  completeThrough: '2026-10-26',
  data: [
    {
      date: '2026-10-25',
      values: {
        unused: '50',
        used: '50',
      },
    },
    {
      date: '2026-10-26',
      values: {
        unused: '60',
        used: '60',
      },
    },
  ],
  series: [
    {
      color: '#f59e0b',
      key: 'used',
      label: 'Used',
    },
    {
      color: '#06b6d4',
      key: 'unused',
      label: 'Unused',
    },
  ],
  source: 'indexedChainTimeUtc8',
  title: 'Calendar Capacity History',
} satisfies AssetCapacityHistory;
export const sporeCapacityHistoryProducer = {
  anchor: {
    blockHash: '0x0202020202020202020202020202020202020202020202020202020202020202',
    blockNumber: 2,
    chainTime: '1793059200000',
  },
  completeThrough: '2026-10-26',
  data: [
    {
      date: '2026-10-25',
      values: {
        unused: '50',
        used: '50',
      },
    },
    {
      date: '2026-10-26',
      values: {
        unused: '60',
        used: '60',
      },
    },
  ],
  series: [
    {
      color: '#f59e0b',
      key: 'used',
      label: 'Used',
    },
    {
      color: '#06b6d4',
      key: 'unused',
      label: 'Unused',
    },
  ],
  source: 'indexedChainTimeUtc8',
  title: 'Spore Capacity History',
} satisfies AssetCapacityHistory;
export const objectCollectionCapacityHistoryProducer = {
  anchor: {
    blockHash: '0x0202020202020202020202020202020202020202020202020202020202020202',
    blockNumber: 2,
    chainTime: '1793059200000',
  },
  completeThrough: '2026-10-26',
  data: [
    {
      date: '2026-10-25',
      values: {
        unused: '50',
        used: '50',
      },
    },
    {
      date: '2026-10-26',
      values: {
        unused: '60',
        used: '60',
      },
    },
  ],
  series: [
    {
      color: '#f59e0b',
      key: 'used',
      label: 'Used',
    },
    {
      color: '#00c389',
      key: 'unused',
      label: 'Unused',
    },
  ],
  source: 'indexedChainTimeUtc8',
  title: '0x7777777777777777777777777777777777777777777777777777777777777777 Capacity History',
} satisfies AssetCapacityHistory;
export const bitCellLegacyProtocolProducer = {
  itemId: '0x2121212121212121212121212121212121212121212121212121212121212121',
  kind: 'bit_cell',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 61,
          lockBytes: 53,
          typeBytes: 65,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0xd517c23c21f4a5675a97ce6a58343bbfac4e3c042e119e90564d1caf7791c20f',
        isLive: true,
        lock: {
          args: '0x0101010101010101010101010101010101010101',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '18700000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
        ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
        timestamp: '1700000000100',
        txHash: '0x3e35fe5ad7951ccb966d3d00682751aa608b63aeb380014dfe71d1628ac90b23',
        txIndex: 0,
        type: {
          args: '0x2121212121212121212121212121212121212121212121212121212121212121',
          codeHash: '0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x000000003900000010000000240000002c000000444444444444444444444444444444444444444400b33f710000000009000000616c6963652e626974',
      decoded: {
        account: 'alice.bit',
        accountId: '0x631b3f98b26fcf4ce7d034e976d56c96cb81b044',
        expiredAt: '1900000000',
        identityId: '0x2121212121212121212121212121212121212121212121212121212121212121',
        linkedAccount: null,
        linkedAccountSource: 'currentCanonicalAccount',
      },
      fields: [
        {
          end: 40,
          name: 'witnessHash',
          start: 20,
          valueHex: '0x4444444444444444444444444444444444444444',
        },
        {
          end: 48,
          name: 'expiredAt',
          start: 40,
          valueHex: '0x00b33f7100000000',
        },
        {
          end: 61,
          name: 'account',
          start: 48,
          valueHex: '0x09000000616c6963652e626974',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const bitCellProtocolProducer = {
  itemId: '0x2222222222222222222222222222222222222222222222222222222222222222',
  kind: 'bit_cell',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 78,
          lockBytes: 53,
          typeBytes: 65,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0xc3dcac6421c772632233b049ca307a87c6abd5cc6556850fe14f1d1606401fc2',
        isLive: true,
        lock: {
          args: '0x0101010101010101010101010101010101010101',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '20400000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
        ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
        timestamp: '1700000000100',
        txHash: '0xbf0608f4099ba088540c4eb7a1176e55a77d9fcf20266e0d907631c68e042732',
        txIndex: 0,
        type: {
          args: '0x2222222222222222222222222222222222222222222222222222222222222222',
          codeHash: '0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x4e00000010000000230000004e0000000f0000006170706c69636174696f6e2f646964270000000001444444444444444444444444444444444444444400b33f7100000000616c6963652e626974',
      decoded: {
        account: 'alice.bit',
        accountId: '0x631b3f98b26fcf4ce7d034e976d56c96cb81b044',
        expiredAt: '1900000000',
        identityId: '0x2222222222222222222222222222222222222222222222222222222222222222',
        linkedAccount: null,
        linkedAccountSource: 'currentCanonicalAccount',
      },
      fields: [
        {
          end: 40,
          name: 'prefix',
          start: 39,
          valueHex: '0x00',
        },
        {
          end: 41,
          name: 'version',
          start: 40,
          valueHex: '0x01',
        },
        {
          end: 61,
          name: 'witnessHash',
          start: 41,
          valueHex: '0x4444444444444444444444444444444444444444',
        },
        {
          end: 69,
          name: 'expiredAt',
          start: 61,
          valueHex: '0x00b33f7100000000',
        },
        {
          end: 78,
          name: 'account',
          start: 69,
          valueHex: '0x616c6963652e626974',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const didProtocolProducer = {
  itemId: '0x1111111111111111111111111111111111111111111111111111111111111111',
  kind: 'did_ckb',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 239,
          lockBytes: 53,
          typeBytes: 65,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0xedfd1c4f99f5af85d0b1af67ea0cf8c4dd108fb07ff6f5e80d7a89ba0edfb6e3',
        isLive: true,
        lock: {
          args: '0x0101010101010101010101010101010101010101',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '36500000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
        ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
        timestamp: '1700000000100',
        txHash: '0x4182b486dc5250570e5e4f0e11e8dbb243bf90674ee99f7bc941d52642e6a996',
        txIndex: 0,
        type: {
          args: '0x1111111111111111111111111111111111111111111111111111111111111111',
          codeHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x00000000eb0000000c000000e0000000d0000000a46b616c736f4b6e6f776e4173816f61743a2f2f616c6963652e746573746673657269616c1bffffffffffffffff677365727669636581a26f73657276696365456e64706f696e747368747470733a2f2f7064732e6578616d706c6564747970657819417470726f746f506572736f6e616c44617461536572766572687365727669636573a16b617470726f746f5f706473a268656e64706f696e747368747470733a2f2f7064732e6578616d706c6564747970657819417470726f746f506572736f6e616c44617461536572766572070000006c6f63616c2d31',
      decoded: {
        did: 'did:ckb:ceirceirceirceirceirceirceirceir',
        document: {
          alsoKnownAs: ['at://alice.test'],
          serial: {
            $integer: '18446744073709551615',
          },
          service: [
            {
              serviceEndpoint: 'https://pds.example',
              type: 'AtprotoPersonalDataServer',
            },
          ],
          services: {
            atproto_pds: {
              endpoint: 'https://pds.example',
              type: 'AtprotoPersonalDataServer',
            },
          },
        },
        documentCborHex:
          '0xa46b616c736f4b6e6f776e4173816f61743a2f2f616c6963652e746573746673657269616c1bffffffffffffffff677365727669636581a26f73657276696365456e64706f696e747368747470733a2f2f7064732e6578616d706c6564747970657819417470726f746f506572736f6e616c44617461536572766572687365727669636573a16b617470726f746f5f706473a268656e64706f696e747368747470733a2f2f7064732e6578616d706c6564747970657819417470726f746f506572736f6e616c44617461536572766572',
        documentEncoding: 'cborLosslessJson',
        documentVersion: 1,
        itemId: '0x1111111111111111111111111111111111111111111111111111111111111111',
        localId: 'local-1',
        pdsEndpoints: ['https://pds.example'],
      },
      fields: [
        {
          end: 4,
          name: 'union',
          start: 0,
          valueHex: '0x00000000',
        },
        {
          end: 228,
          name: 'document',
          start: 20,
          valueHex:
            '0xa46b616c736f4b6e6f776e4173816f61743a2f2f616c6963652e746573746673657269616c1bffffffffffffffff677365727669636581a26f73657276696365456e64706f696e747368747470733a2f2f7064732e6578616d706c6564747970657819417470726f746f506572736f6e616c44617461536572766572687365727669636573a16b617470726f746f5f706473a268656e64706f696e747368747470733a2f2f7064732e6578616d706c6564747970657819417470726f746f506572736f6e616c44617461536572766572',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const dotbitV1ProtocolProducer = {
  itemId: '0x3333333333333333333333333333333333333333',
  kind: 'dotbit',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 80,
          lockBytes: 87,
          typeBytes: 53,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0x62e8461bf3d5c88594c63739e0ee82e7400a8dbd064142225b2f4b84903fd34c',
        isLive: true,
        lock: {
          args: '0x036666666666666666666666666666666666666666067777777777777777777777777777777777777777777777777777777777777777',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '22800000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgrvenxvenxvenxvenxvenxvenxvenxvenxqemhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwyz4vmj',
        ownerLockHash: '0x4c1b4df29a210d377ff56c59442fc017dd45fe91f2a405e99375d0682adccd33',
        timestamp: '1700000000100',
        txHash: '0x761920f1f9323f0ce1634de7fea42cbcc4520bdb4d43e9e190b0a77cac849c16',
        txIndex: 0,
        type: {
          args: '0x3333333333333333333333333333333333333333',
          codeHash: '0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0xd0089771ee63a23a7d47c902b68fde91b665edd76cb18e8d092d16a5c3ae16bc3333333333333333333333333333333333333333555555555555555555555555555555555555555500b33f7100000000',
      decoded: {
        account: 'alice.bit',
        accountId: '0x3333333333333333333333333333333333333333',
        expiredAt: '1900000000',
        manager: {
          algorithm: 6,
          argsHex: '0x7777777777777777777777777777777777777777777777777777777777777777',
        },
        nextAccountId: '0x5555555555555555555555555555555555555555',
        nextAccountName: null,
        owner: {
          algorithm: 3,
          argsHex: '0x6666666666666666666666666666666666666666',
        },
        records: [
          {
            key: '60',
            label: 'main',
            ttl: 300,
            type: 'address',
            value: '0xabc',
            valueHex: '0x3078616263',
          },
        ],
        registeredAt: '1600000000',
        status: 0,
        witnessVersion: 1,
      },
      fields: [
        {
          end: 32,
          name: 'accountDataHash',
          start: 0,
          valueHex: '0xd0089771ee63a23a7d47c902b68fde91b665edd76cb18e8d092d16a5c3ae16bc',
        },
        {
          end: 52,
          name: 'accountId',
          start: 32,
          valueHex: '0x3333333333333333333333333333333333333333',
        },
        {
          end: 72,
          name: 'nextAccountId',
          start: 52,
          valueHex: '0x5555555555555555555555555555555555555555',
        },
        {
          end: 80,
          name: 'expiredAt',
          start: 72,
          valueHex: '0x00b33f7100000000',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const dotcellProtocolProducer = {
  itemId: '0x5bc4234fbb5f80f9dc8382b5e4051922815e4f38',
  kind: 'dotcell',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 103,
          lockBytes: 53,
          typeBytes: 53,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0x8e8cef863521c2b960acf88ea83ec09bec9ebda7a59c06b9abd0b3c291b2b949',
        isLive: true,
        lock: {
          args: '0x0101010101010101010101010101010101010101',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '21700000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
        ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
        timestamp: '1700000000100',
        txHash: '0xe5f008d482b4e40667ea4b44736c721ba94db39fea001dcedb986e555d1fd5ef',
        txIndex: 0,
        type: {
          args: '0x5bc4234fbb5f80f9dc8382b5e4051922815e4f38',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2000000000000000000000000000000000000000000b33f710061616161616161616161616161616161616161616262626262626262626262626262626262626262616c696365',
      decoded: {
        expiredAt: '1900000000',
        label: 'alice',
        layoutVersion: 3,
        manager: {
          address: null,
          hashPrefix: '0x6262626262626262626262626262626262626262',
          lockHash: null,
          scriptName: null,
        },
        name: 'alice.cell',
        nextId: '0x0000000000000000000000000000000000000000',
        owner: {
          address: null,
          hashPrefix: '0x6161616161616161616161616161616161616161',
          lockHash: null,
          scriptName: null,
        },
        records: [],
        recordsHash: '0x72ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2',
      },
      fields: [
        {
          end: 1,
          name: 'layoutVersion',
          start: 0,
          valueHex: '0x03',
        },
        {
          end: 33,
          name: 'recordsHash',
          start: 1,
          valueHex: '0x72ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2',
        },
        {
          end: 53,
          name: 'nextId',
          start: 33,
          valueHex: '0x0000000000000000000000000000000000000000',
        },
        {
          end: 58,
          name: 'expiredAt',
          start: 53,
          valueHex: '0x00b33f7100',
        },
        {
          end: 78,
          name: 'owner',
          start: 58,
          valueHex: '0x6161616161616161616161616161616161616161',
        },
        {
          end: 98,
          name: 'manager',
          start: 78,
          valueHex: '0x6262626262626262626262626262626262626262',
        },
        {
          end: 103,
          name: 'label',
          start: 98,
          valueHex: '0x616c696365',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const dotbitV2ProtocolProducer = {
  itemId: '0x3333333333333333333333333333333333333333',
  kind: 'dotbit',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 80,
          lockBytes: 87,
          typeBytes: 53,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0x730cdae14f18d001e2fa05bbd427291a9124460fda5bb1b682fcaa3353122779',
        isLive: true,
        lock: {
          args: '0x036666666666666666666666666666666666666666067777777777777777777777777777777777777777777777777777777777777777',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '22800000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgrvenxvenxvenxvenxvenxvenxvenxvenxqemhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwyz4vmj',
        ownerLockHash: '0x4c1b4df29a210d377ff56c59442fc017dd45fe91f2a405e99375d0682adccd33',
        timestamp: '1700000000100',
        txHash: '0x107081f2287e4f392600f2c4da59e1e7a07929a7ffb6d93aa834092b8a94869c',
        txIndex: 0,
        type: {
          args: '0x3333333333333333333333333333333333333333',
          codeHash: '0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x6bfa65e4e16d241405ed782a343491407c56f552d3f2847330afd77a00d96aa03333333333333333333333333333333333333333555555555555555555555555555555555555555500b33f7100000000',
      decoded: {
        account: 'alice.bit',
        accountId: '0x3333333333333333333333333333333333333333',
        expiredAt: '1900000000',
        manager: {
          algorithm: 6,
          argsHex: '0x7777777777777777777777777777777777777777777777777777777777777777',
        },
        nextAccountId: '0x5555555555555555555555555555555555555555',
        nextAccountName: null,
        owner: {
          algorithm: 3,
          argsHex: '0x6666666666666666666666666666666666666666',
        },
        records: [
          {
            key: '60',
            label: 'main',
            ttl: 300,
            type: 'address',
            value: '0xabc',
            valueHex: '0x3078616263',
          },
        ],
        registeredAt: '1600000000',
        status: 0,
        witnessVersion: 2,
      },
      fields: [
        {
          end: 32,
          name: 'accountDataHash',
          start: 0,
          valueHex: '0x6bfa65e4e16d241405ed782a343491407c56f552d3f2847330afd77a00d96aa0',
        },
        {
          end: 52,
          name: 'accountId',
          start: 32,
          valueHex: '0x3333333333333333333333333333333333333333',
        },
        {
          end: 72,
          name: 'nextAccountId',
          start: 52,
          valueHex: '0x5555555555555555555555555555555555555555',
        },
        {
          end: 80,
          name: 'expiredAt',
          start: 72,
          valueHex: '0x00b33f7100000000',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const mnftProtocolProducer = {
  itemId: '0x12121212121212121212121212121212121212121212121212121212',
  kind: 'm-nft',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 11,
          lockBytes: 53,
          typeBytes: 61,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0xef24307552a4621ea83ab0726d1e2658dcf8f628643816aa0e2da26c6131040d',
        isLive: true,
        lock: {
          args: '0x0101010101010101010101010101010101010101',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '13300000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
        ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
        timestamp: '1700000000100',
        txHash: '0xf265f5e569b6d0ee919016037ec907b63c77f54a40219442ac6e3d967771beaa',
        txIndex: 0,
        type: {
          args: '0x12121212121212121212121212121212121212121212121212121212',
          codeHash: '0x2b24f0d644ccbdd77bbf86b27c8cca02efa0ad051e447c212636d9ee7acaaec9',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex: '0x009898989898989898c003',
      decoded: {
        characteristicHex: '0x9898989898989898',
        claimed: true,
        configure: 192,
        locked: true,
        permissions: {
          allowCharacteristicUpdate: true,
          allowClaim: true,
          allowDestroyAfterClaim: false,
          allowDestroyBeforeClaim: false,
          allowExtension: true,
          allowLock: true,
          allowTransferAfterClaim: true,
          allowTransferBeforeClaim: true,
        },
        state: 3,
      },
      fields: [
        {
          end: 1,
          name: 'version',
          start: 0,
          valueHex: '0x00',
        },
        {
          end: 9,
          name: 'characteristic',
          start: 1,
          valueHex: '0x9898989898989898',
        },
        {
          end: 10,
          name: 'configure',
          start: 9,
          valueHex: '0xc0',
        },
        {
          end: 11,
          name: 'state',
          start: 10,
          valueHex: '0x03',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const dotbitV3ProtocolProducer = {
  itemId: '0x3333333333333333333333333333333333333333',
  kind: 'dotbit',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 80,
          lockBytes: 87,
          typeBytes: 53,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0x6e7cad7a2539aa7037b197b0faf22261b984e182916f9a7fccc1f398f3a0e2c5',
        isLive: true,
        lock: {
          args: '0x036666666666666666666666666666666666666666067777777777777777777777777777777777777777777777777777777777777777',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '22800000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgrvenxvenxvenxvenxvenxvenxvenxvenxqemhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwyz4vmj',
        ownerLockHash: '0x4c1b4df29a210d377ff56c59442fc017dd45fe91f2a405e99375d0682adccd33',
        timestamp: '1700000000100',
        txHash: '0x7db0dd6ce39d57008941916f273885cbb4969f71b1dadae02c779b4e9e3dac40',
        txIndex: 0,
        type: {
          args: '0x3333333333333333333333333333333333333333',
          codeHash: '0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0xb10c79ea3452baa1a2163afd49522dd59238da076d81222e7ddf51747cb30a9b3333333333333333333333333333333333333333555555555555555555555555555555555555555500b33f7100000000',
      decoded: {
        account: 'alice.bit',
        accountId: '0x3333333333333333333333333333333333333333',
        expiredAt: '1900000000',
        manager: {
          algorithm: 6,
          argsHex: '0x7777777777777777777777777777777777777777777777777777777777777777',
        },
        nextAccountId: '0x5555555555555555555555555555555555555555',
        nextAccountName: null,
        owner: {
          algorithm: 3,
          argsHex: '0x6666666666666666666666666666666666666666',
        },
        records: [
          {
            key: '60',
            label: 'main',
            ttl: 300,
            type: 'address',
            value: '0xabc',
            valueHex: '0x3078616263',
          },
        ],
        registeredAt: '1600000000',
        status: 0,
        witnessVersion: 3,
      },
      fields: [
        {
          end: 32,
          name: 'accountDataHash',
          start: 0,
          valueHex: '0xb10c79ea3452baa1a2163afd49522dd59238da076d81222e7ddf51747cb30a9b',
        },
        {
          end: 52,
          name: 'accountId',
          start: 32,
          valueHex: '0x3333333333333333333333333333333333333333',
        },
        {
          end: 72,
          name: 'nextAccountId',
          start: 52,
          valueHex: '0x5555555555555555555555555555555555555555',
        },
        {
          end: 80,
          name: 'expiredAt',
          start: 72,
          valueHex: '0x00b33f7100000000',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const dotbitV4ProtocolProducer = {
  itemId: '0x3333333333333333333333333333333333333333',
  kind: 'dotbit',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 80,
          lockBytes: 87,
          typeBytes: 53,
        },
        blockNumber: 100,
        capacity: '100000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0x55b3aa2474f6fe1dbd3947a52aaa1cdda7e3c1c74ff810fb3225a49218a44f66',
        isLive: true,
        lock: {
          args: '0x036666666666666666666666666666666666666666067777777777777777777777777777777777777777777777777777777777777777',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '22800000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgrvenxvenxvenxvenxvenxvenxvenxvenxqemhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwamhwyz4vmj',
        ownerLockHash: '0x4c1b4df29a210d377ff56c59442fc017dd45fe91f2a405e99375d0682adccd33',
        timestamp: '1700000000100',
        txHash: '0xd4537ec3c91c87df3382c6a2628fbcbcd52d06798e2ea0a89f31a3498ec709b5',
        txIndex: 0,
        type: {
          args: '0x3333333333333333333333333333333333333333',
          codeHash: '0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x2dc4f7670ed5a31cc8f3d52db9937e498abd71b281679d0c3df46d2d36d71ff43333333333333333333333333333333333333333555555555555555555555555555555555555555500b33f7100000000',
      decoded: {
        account: 'alice.bit',
        accountId: '0x3333333333333333333333333333333333333333',
        expiredAt: '1900000000',
        manager: {
          algorithm: 6,
          argsHex: '0x7777777777777777777777777777777777777777777777777777777777777777',
        },
        nextAccountId: '0x5555555555555555555555555555555555555555',
        nextAccountName: null,
        owner: {
          algorithm: 3,
          argsHex: '0x6666666666666666666666666666666666666666',
        },
        records: [
          {
            key: '60',
            label: 'main',
            ttl: 300,
            type: 'address',
            value: '0xabc',
            valueHex: '0x3078616263',
          },
        ],
        registeredAt: '1600000000',
        status: 0,
        witnessVersion: 4,
      },
      fields: [
        {
          end: 32,
          name: 'accountDataHash',
          start: 0,
          valueHex: '0x2dc4f7670ed5a31cc8f3d52db9937e498abd71b281679d0c3df46d2d36d71ff4',
        },
        {
          end: 52,
          name: 'accountId',
          start: 32,
          valueHex: '0x3333333333333333333333333333333333333333',
        },
        {
          end: 72,
          name: 'nextAccountId',
          start: 52,
          valueHex: '0x5555555555555555555555555555555555555555',
        },
        {
          end: 80,
          name: 'expiredAt',
          start: 72,
          valueHex: '0x00b33f7100000000',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
export const sporeProtocolProducer = {
  itemId: '0x0202020202020202020202020202020202020202020202020202020202020202',
  kind: 'spore',
  versions: [
    {
      cell: {
        anatomy: {
          capacityBytes: 8,
          dataBytes: 103,
          lockBytes: 53,
          typeBytes: 65,
        },
        blockNumber: 100,
        capacity: '50000000000',
        consumedAtBlock: null,
        consumedByTx: null,
        dataHash: '0xc9d057d5666780ce964fba028388f3e4318803591c90327e7be9fc4d5568d002',
        isLive: true,
        lock: {
          args: '0x0101010101010101010101010101010101010101',
          codeHash: '0x4141414141414141414141414141414141414141414141414141414141414141',
          hashType: 'type',
        },
        lockName: null,
        occupiedCapacity: '22900000000',
        outputIndex: 0,
        ownerAddress:
          'ckb1qpq5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zs2pg9q5zqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgutsyjq',
        ownerLockHash: '0x5e917826cb528fa25e9de839dbed4358e230de466bae590758efb7d481f8ab6c',
        timestamp: '1700000000100',
        txHash: '0x476dac1106cc14a9bf59b6d3517bc04bf93cec42ae633bb731bc1757b933b7f9',
        txIndex: 1,
        type: {
          args: '0x0202020202020202020202020202020202020202020202020202020202020202',
          codeHash: '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
          hashType: 'type',
        },
        typeName: null,
      },
      dataHex:
        '0x6700000010000000190000004300000005000000646f622f30260000007b22646e61223a223031222c226964223a31383434363734343037333730393535313631347d200000007777777777777777777777777777777777777777777777777777777777777777',
      decoded: {
        contentHex:
          '0x7b22646e61223a223031222c226964223a31383434363734343037333730393535313631347d',
        dnaHex: '0x01',
      },
      fields: [
        {
          end: 25,
          name: 'contentType',
          start: 16,
          valueHex: '0x05000000646f622f30',
        },
        {
          end: 67,
          name: 'content',
          start: 25,
          valueHex:
            '0x260000007b22646e61223a223031222c226964223a31383434363734343037333730393535313631347d',
        },
        {
          end: 103,
          name: 'clusterId',
          start: 67,
          valueHex: '0x200000007777777777777777777777777777777777777777777777777777777777777777',
        },
      ],
    },
  ],
} satisfies ItemProtocol;
