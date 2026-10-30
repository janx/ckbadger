// Built from the board data dumps of Nervape 3x (mockup/data-object, captured from the API on
// 2026-10-25): the object 0x51c75ad6…96f2, its DOB pattern, and the traits of all 90 objects.
import type { CollectionInsights, ObjectGenetics } from '@/lib/api';
export const NERVAPE3X_ID = '0x51c75ad6c509e05ec5933e32093a5b32eeab7fe59dbe9ecc05546306002796f2';
export const NERVAPE3X_CLUSTER =
  '0x26a1fe3f4e84ae2cd1dc3e319ca416e27624c5ebe2f1b0aa731ca18d67784c4c';
export const nervape3xGenetics = {
  dnaHex:
    '0xa94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0033358303132354e4d4e5641503033333230320000',
  number: null,
  cachedVmTraits: {
    status: 'decoded',
    decodedAt: '1793261917',
    traits: [
      {
        name: 'prev.type',
        value: 'image',
      },
      {
        name: 'relief',
        value: 'NERVAPE 3X',
      },
      {
        name: 'prev.bg',
        value: 'btcfs://a94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0i0',
      },
      {
        name: 'Color',
        value: 'Nature Green',
      },
      {
        name: 'Serial',
        value: '3X0125NMNVAP033202',
      },
      {
        name: 'Note',
        value: '',
      },
    ],
    source: 'cachedVm',
    originalDecoderHashRecorded: false,
  },
  currentPattern: {
    status: 'dob',
    version: 0,
    source: 'currentCanonicalDecoder',
    steps: [
      {
        step: 0,
        decoderReference: {
          type: 'type_id',
          hash: '0xf9e6e61b29ef7356cdacce217065bc1ec7ec95cf33f62d2c800b913a66688e3d',
        },
        decoderBinaryHash: null,
        description: {
          status: 'supported',
          format: 'nervapeJsonDob0',
          rangePolicy: 'clipToDna',
          genes: [
            {
              index: 0,
              traitIndex: 0,
              valueIndex: 0,
              name: 'prev.type',
              typeTag: 'String',
              byteOffset: '0',
              byteLength: '1',
              rule: 'options',
              ruleArguments: ['image'],
            },
            {
              index: 1,
              traitIndex: 1,
              valueIndex: 0,
              name: 'relief',
              typeTag: 'String',
              byteOffset: '0',
              byteLength: '1',
              rule: 'options',
              ruleArguments: ['NERVAPE 3X'],
            },
            {
              index: 2,
              traitIndex: 2,
              valueIndex: 0,
              name: 'prev.bg',
              typeTag: 'String',
              byteOffset: '0',
              byteLength: '32',
              rule: 'btcfs',
              ruleArguments: null,
            },
            {
              index: 3,
              traitIndex: 3,
              valueIndex: 0,
              name: 'Color',
              typeTag: 'String',
              byteOffset: '32',
              byteLength: '1',
              rule: 'nervapeColor',
              ruleArguments: null,
            },
            {
              index: 4,
              traitIndex: 4,
              valueIndex: 0,
              name: 'Serial',
              typeTag: 'String',
              byteOffset: '33',
              byteLength: '18',
              rule: 'utf8',
              ruleArguments: null,
            },
            {
              index: 5,
              traitIndex: 5,
              valueIndex: 0,
              name: 'Note',
              typeTag: 'String',
              byteOffset: '51',
              byteLength: '2',
              rule: 'nervapeNote',
              ruleArguments: null,
            },
          ],
        },
        bytes: [
          {
            geneIndex: 0,
            declaredByteOffset: '0',
            declaredByteLength: '1',
            byteOffset: 0,
            byteLength: 1,
            truncated: false,
            rawHex: 'a9',
          },
          {
            geneIndex: 1,
            declaredByteOffset: '0',
            declaredByteLength: '1',
            byteOffset: 0,
            byteLength: 1,
            truncated: false,
            rawHex: 'a9',
          },
          {
            geneIndex: 2,
            declaredByteOffset: '0',
            declaredByteLength: '32',
            byteOffset: 0,
            byteLength: 32,
            truncated: false,
            rawHex: 'a94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0',
          },
          {
            geneIndex: 3,
            declaredByteOffset: '32',
            declaredByteLength: '1',
            byteOffset: 32,
            byteLength: 1,
            truncated: false,
            rawHex: '03',
          },
          {
            geneIndex: 4,
            declaredByteOffset: '33',
            declaredByteLength: '18',
            byteOffset: 33,
            byteLength: 18,
            truncated: false,
            rawHex: '3358303132354e4d4e564150303333323032',
          },
          {
            geneIndex: 5,
            declaredByteOffset: '51',
            declaredByteLength: '2',
            byteOffset: 51,
            byteLength: 2,
            truncated: false,
            rawHex: '0000',
          },
        ],
      },
    ],
  },
} satisfies ObjectGenetics;
export const nervape3xInsights = {
  completeThrough: '2026-10-24',
  calendarAnchor: null,
  calendarSource: 'indexedChainTimeUtc8',
  holderDistribution: [
    {
      range: '1',
      holders: 90,
      items: 90,
    },
    {
      range: '2',
      holders: 0,
      items: 0,
    },
    {
      range: '3\u20135',
      holders: 0,
      items: 0,
    },
    {
      range: '6+',
      holders: 0,
      items: 0,
    },
  ],
  weeklyActivity: [
    {
      weekStart: '2026-10-19',
      mintTransactions: 90,
      transferTransactions: 94,
      burnTransactions: 0,
    },
  ],
  genetics: {
    liveItems: 90,
    eligibleItems: 90,
    nonDobItems: 0,
    decodedItems: 90,
    pendingItems: 0,
    failedItems: 0,
    complete: true,
    traits: [
      {
        name: 'Color',
        values: [
          {
            value: 'Blush Pink',
            count: 10,
          },
          {
            value: 'Calm Blue',
            count: 15,
          },
          {
            value: 'Fortune Gold',
            count: 15,
          },
          {
            value: 'Lucky Lavender',
            count: 5,
          },
          {
            value: 'Nature Green',
            count: 10,
          },
          {
            value: 'Pure White',
            count: 5,
          },
          {
            value: 'Thai Special',
            count: 10,
          },
          {
            value: 'Water Veins',
            count: 20,
          },
        ],
      },
      {
        name: 'Note',
        values: [
          {
            value: '',
            count: 90,
          },
        ],
      },
      {
        name: 'Serial',
        values: [
          {
            value: '\u0000\u0002\u00003X0125CLNVAP011',
            count: 5,
          },
          {
            value: '\u0000\u0002\u00003X0125CLNVAP014',
            count: 10,
          },
          {
            value: '\u0000\u0002\u00003X0125CLNVAP016',
            count: 5,
          },
          {
            value: '3X0125CLNVAP000201',
            count: 1,
          },
          {
            value: '3X0125CLNVAP000202',
            count: 1,
          },
          {
            value: '3X0125CLNVAP000203',
            count: 1,
          },
          {
            value: '3X0125CLNVAP000204',
            count: 1,
          },
          {
            value: '3X0125CLNVAP000205',
            count: 1,
          },
          {
            value: '3X0125CLNVAP002101',
            count: 1,
          },
          {
            value: '3X0125CLNVAP002102',
            count: 1,
          },
          {
            value: '3X0125CLNVAP002103',
            count: 1,
          },
          {
            value: '3X0125CLNVAP002104',
            count: 1,
          },
          {
            value: '3X0125CLNVAP002105',
            count: 1,
          },
          {
            value: '3X0125NMNVAP000101',
            count: 1,
          },
          {
            value: '3X0125NMNVAP000102',
            count: 1,
          },
          {
            value: '3X0125NMNVAP000103',
            count: 1,
          },
          {
            value: '3X0125NMNVAP000104',
            count: 1,
          },
          {
            value: '3X0125NMNVAP000105',
            count: 1,
          },
          {
            value: '3X0125NMNVAP010801',
            count: 1,
          },
          {
            value: '3X0125NMNVAP010802',
            count: 1,
          },
          {
            value: '3X0125NMNVAP010803',
            count: 1,
          },
          {
            value: '3X0125NMNVAP010804',
            count: 1,
          },
          {
            value: '3X0125NMNVAP010805',
            count: 1,
          },
          {
            value: '3X0125NMNVAP011201',
            count: 1,
          },
          {
            value: '3X0125NMNVAP011202',
            count: 1,
          },
          {
            value: '3X0125NMNVAP011203',
            count: 1,
          },
          {
            value: '3X0125NMNVAP011204',
            count: 1,
          },
          {
            value: '3X0125NMNVAP011205',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020001',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020002',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020003',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020004',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020005',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020101',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020102',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020103',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020104',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020105',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020201',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020202',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020203',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020204',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020205',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020701',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020702',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020703',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020704',
            count: 1,
          },
          {
            value: '3X0125NMNVAP020705',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033201',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033202',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033203',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033204',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033205',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033401',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033402',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033403',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033404',
            count: 1,
          },
          {
            value: '3X0125NMNVAP033405',
            count: 1,
          },
          {
            value: '3X0125NMNVAP040401',
            count: 1,
          },
          {
            value: '3X0125NMNVAP040402',
            count: 1,
          },
          {
            value: '3X0125NMNVAP040403',
            count: 1,
          },
          {
            value: '3X0125NMNVAP040404',
            count: 1,
          },
          {
            value: '3X0125NMNVAP040405',
            count: 1,
          },
          {
            value: '3X0125NMNVAP050101',
            count: 1,
          },
          {
            value: '3X0125NMNVAP050102',
            count: 1,
          },
          {
            value: '3X0125NMNVAP050103',
            count: 1,
          },
          {
            value: '3X0125NMNVAP050104',
            count: 1,
          },
          {
            value: '3X0125NMNVAP050105',
            count: 1,
          },
          {
            value: '3X0125NMNVAP051201',
            count: 1,
          },
          {
            value: '3X0125NMNVAP051202',
            count: 1,
          },
          {
            value: '3X0125NMNVAP051203',
            count: 1,
          },
          {
            value: '3X0125NMNVAP051204',
            count: 1,
          },
          {
            value: '3X0125NMNVAP051205',
            count: 1,
          },
        ],
      },
      {
        name: 'prev.bg',
        values: [
          {
            value: 'btcfs://0bfb23cc12eff888099b07b423632c60d2ef879a08cc02bffb9595acee2c5719i0',
            count: 2,
          },
          {
            value: 'btcfs://1037617808df74043886c95d3a98ef95a06fd22ed9fad10e9cdcf9664fee6339i0',
            count: 2,
          },
          {
            value: 'btcfs://15edef226a2da369994da641bc34c7fc71376a2d97ba27a1205b5fc43eb33867i0',
            count: 4,
          },
          {
            value: 'btcfs://1bed5592c9ed7dd76450709c50548fbd8bbe5a08d7937a7fcba31bbb29380a5ci0',
            count: 4,
          },
          {
            value: 'btcfs://24751aefc75c23892386c9df4f5acb1a95487b7c922a827a613205513b73bec2i0',
            count: 3,
          },
          {
            value: 'btcfs://3360e6646332d802dea426eece85601b258bd8083af3fac1af48800d0e14013ei0',
            count: 1,
          },
          {
            value: 'btcfs://3ccc45f0ae6190a843249661a5e45aeec02ad663e355bc4687b7a4ae175a03d5i0',
            count: 2,
          },
          {
            value: 'btcfs://4829eb09d16ab133c407b56363299ee98d127c095bf22f8b4746dfad9416d920i0',
            count: 1,
          },
          {
            value: 'btcfs://4aa4831f9a2452bbfcc7651667e615454b7cd52261d7816315271e228f15855ai0',
            count: 1,
          },
          {
            value: 'btcfs://4e34fe289607dee70ffa2b740ed4419785c0cbc72b3d5daa1cffc8b626c06f5fi0',
            count: 1,
          },
          {
            value: 'btcfs://518aa57ddee7a00263b4424ff5b5986962a25645842677e9bdfc98201c5a2228i0',
            count: 2,
          },
          {
            value: 'btcfs://5397d7463ace0f49587f63d511ec72a3b82ae7422a828db4a07f57a6aa946eafi0',
            count: 2,
          },
          {
            value: 'btcfs://553335db09679c1fd45ee138fada9737c8d2b747e9802939e0e9d369dc18bb2di0',
            count: 3,
          },
          {
            value: 'btcfs://655cd7b8f1a1664a5ddd99cec00f083f39fb51be7d7d382003257de99b7ca905i0',
            count: 2,
          },
          {
            value: 'btcfs://6a007ba152db4c8c34b57c6fefc07de870bfeaa3016cb67a03f80aaaf71120fci0',
            count: 3,
          },
          {
            value: 'btcfs://6b8d4eabff21265ee645b713d29bcf404675bbee291b6803b161b839ba485312i0',
            count: 3,
          },
          {
            value: 'btcfs://6cd38d8074a974b8068aadf8d539baf858db88528330b1cb82fcdb53d9ff7f04i0',
            count: 3,
          },
          {
            value: 'btcfs://703e401c817adfb538a0f071b5b433470edb9ba0e2364c4c3ef0b85ef00e1726i0',
            count: 2,
          },
          {
            value: 'btcfs://748890596a351db8bc445790880a04a707862fad8b31f193f0ff056c281c1ad0i0',
            count: 2,
          },
          {
            value: 'btcfs://7b6309fb0c60f4cfa67f6580fb06169959eb015008a34e7a2787c914bf956c37i0',
            count: 1,
          },
          {
            value: 'btcfs://7d26b8a119ed9f2a009dc69dfd7c87731eb1ac66ce61b8f2fb3a893b1869ca9fi0',
            count: 2,
          },
          {
            value: 'btcfs://8b22567d7331d28a8c1ebed54b7c15544552a3265e256f84d781d68071e73adbi0',
            count: 1,
          },
          {
            value: 'btcfs://8bfa7d8a948f75cd82fc91c535721be8871e578bb32e34f72bfc87e2284c4febi0',
            count: 1,
          },
          {
            value: 'btcfs://94cbc00446988d1a25583db07fe63773885ca430a72faea3d33f8b31eeeecbe4i0',
            count: 2,
          },
          {
            value: 'btcfs://9e74cd0a1a74cee12bc42ed326c5f6858520f82acf77d9b8c8cf18e1adf4ed69i0',
            count: 4,
          },
          {
            value: 'btcfs://9f72f684f0e0c025d8f23376b68ea9ed5b9e65ee42df11325082376b76295375i0',
            count: 3,
          },
          {
            value: 'btcfs://a94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0i0',
            count: 2,
          },
          {
            value: 'btcfs://ac63291a5a380ffb61c0df64715b5cca7791b8a692d4feb367629790e2b33050i0',
            count: 4,
          },
          {
            value: 'btcfs://adecd4a0eea94b23b7e84d12bca444faaefe3d7784387423ef3ca1d9dbf3cb81i0',
            count: 2,
          },
          {
            value: 'btcfs://b11f6de00fd7359028b87daa363aa8c01ebf2c418f90e81bbc52559830985593i0',
            count: 3,
          },
          {
            value: 'btcfs://b70ff98b4a6a8d71030559800939c3f4211c4d05c8d259214fc18eabc2fc21e2i0',
            count: 3,
          },
          {
            value: 'btcfs://c07a7f7ef9549cd40d1169dd6830a0fd0ef0119098bea9bff2f5c24cba1697b4i0',
            count: 3,
          },
          {
            value: 'btcfs://ca2865d1c6b91544e92aeb0a5122b0a843cb5fdc2337ff5ea6d35270ba28b413i0',
            count: 4,
          },
          {
            value: 'btcfs://ca62993423f4ba4fe60ed3850d6c7aefe53448def0430a0a9bcfc71cd7dfe8e3i0',
            count: 1,
          },
          {
            value: 'btcfs://d0d8a95cd533125942ce2de8ab93e5e2aca375afa010a04b11113b1e60db0f43i0',
            count: 2,
          },
          {
            value: 'btcfs://d24916a983b962691e2f22525857de35c35787573f3358e2c873dca316075df3i0',
            count: 1,
          },
          {
            value: 'btcfs://d55ef8c673d132e3836a652d958c674f4b7fdb75a7c69b179486b73672115386i0',
            count: 3,
          },
          {
            value: 'btcfs://e1b31b4e435a82107545b2c32de1e953b19e22c9c7564b9e76259e2d7945af35i0',
            count: 2,
          },
          {
            value: 'btcfs://e38dd40704bafb32bb42578c555717d853200dc9e6254b58704eee4a409ced58i0',
            count: 2,
          },
          {
            value: 'btcfs://f6518a110baf197a0b7c5a972e5bb6e0c244a63ecd6f564c2ed27ba682a9120ai0',
            count: 1,
          },
        ],
      },
      {
        name: 'prev.type',
        values: [
          {
            value: 'image',
            count: 90,
          },
        ],
      },
      {
        name: 'relief',
        values: [
          {
            value: 'NERVAPE 3X',
            count: 90,
          },
        ],
      },
    ],
  },
  currentPattern: {
    status: 'dob',
    version: 0,
    source: 'currentCanonicalDecoder',
    steps: [
      {
        step: 0,
        decoderReference: {
          type: 'type_id',
          hash: '0xf9e6e61b29ef7356cdacce217065bc1ec7ec95cf33f62d2c800b913a66688e3d',
        },
        decoderBinaryHash: null,
        description: {
          status: 'supported',
          format: 'nervapeJsonDob0',
          rangePolicy: 'clipToDna',
          genes: [
            {
              index: 0,
              traitIndex: 0,
              valueIndex: 0,
              name: 'prev.type',
              typeTag: 'String',
              byteOffset: '0',
              byteLength: '1',
              rule: 'options',
              ruleArguments: ['image'],
            },
            {
              index: 1,
              traitIndex: 1,
              valueIndex: 0,
              name: 'relief',
              typeTag: 'String',
              byteOffset: '0',
              byteLength: '1',
              rule: 'options',
              ruleArguments: ['NERVAPE 3X'],
            },
            {
              index: 2,
              traitIndex: 2,
              valueIndex: 0,
              name: 'prev.bg',
              typeTag: 'String',
              byteOffset: '0',
              byteLength: '32',
              rule: 'btcfs',
              ruleArguments: null,
            },
            {
              index: 3,
              traitIndex: 3,
              valueIndex: 0,
              name: 'Color',
              typeTag: 'String',
              byteOffset: '32',
              byteLength: '1',
              rule: 'nervapeColor',
              ruleArguments: null,
            },
            {
              index: 4,
              traitIndex: 4,
              valueIndex: 0,
              name: 'Serial',
              typeTag: 'String',
              byteOffset: '33',
              byteLength: '18',
              rule: 'utf8',
              ruleArguments: null,
            },
            {
              index: 5,
              traitIndex: 5,
              valueIndex: 0,
              name: 'Note',
              typeTag: 'String',
              byteOffset: '51',
              byteLength: '2',
              rule: 'nervapeNote',
              ruleArguments: null,
            },
          ],
        },
        bytes: [],
      },
    ],
  },
} satisfies CollectionInsights;
/** Traits of seven more objects of the collection, keyed by spore id. */
export const nervape3xOtherTraits: Record<string, { name: string; value: string }[]> = {
  '0x71fe552704457c228e3bbe91cd8d45f3e3628c8c825bfc4687792bfe4dc6b103': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://1037617808df74043886c95d3a98ef95a06fd22ed9fad10e9cdcf9664fee6339i0',
    },
    {
      name: 'Color',
      value: 'Nature Green',
    },
    {
      name: 'Serial',
      value: '3X0125NMNVAP033204',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
  '0xfc16d7a5eac3a10b9596d2c2e549be7b3b17fdea217e09d992677335d6b3f918': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://703e401c817adfb538a0f071b5b433470edb9ba0e2364c4c3ef0b85ef00e1726i0',
    },
    {
      name: 'Color',
      value: 'Nature Green',
    },
    {
      name: 'Serial',
      value: '3X0125NMNVAP033205',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
  '0x14c95bfc9d506c289eff8fced17fa586613231c955af361f900022cea28ba1e6': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://7d26b8a119ed9f2a009dc69dfd7c87731eb1ac66ce61b8f2fb3a893b1869ca9fi0',
    },
    {
      name: 'Color',
      value: 'Nature Green',
    },
    {
      name: 'Serial',
      value: '3X0125NMNVAP033203',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
  '0xc1f47d307bfb4bbba774d82533fc6f70069f8fa4d4b241d5a1c2b1fcf2d077b5': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://3ccc45f0ae6190a843249661a5e45aeec02ad663e355bc4687b7a4ae175a03d5i0',
    },
    {
      name: 'Color',
      value: 'Nature Green',
    },
    {
      name: 'Serial',
      value: '3X0125NMNVAP033201',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
  '0x1d0a6e9e509a1b3e1caf6f0c87e83788324629704ad33903ad2223b62bd256b9': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://e1b31b4e435a82107545b2c32de1e953b19e22c9c7564b9e76259e2d7945af35i0',
    },
    {
      name: 'Color',
      value: 'Thai Special',
    },
    {
      name: 'Serial',
      value: '3X0125CLNVAP000202',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
  '0x054e57d5dc1704dbe772a0c39922ff12edeca01e1c823b720cd3e0e1bf474269': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://adecd4a0eea94b23b7e84d12bca444faaefe3d7784387423ef3ca1d9dbf3cb81i0',
    },
    {
      name: 'Color',
      value: 'Thai Special',
    },
    {
      name: 'Serial',
      value: '3X0125CLNVAP000204',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
  '0xf2ff14298d6fa717aafd8a6bdf9d0816165938b9967678fe737f2efb2ae23d27': [
    {
      name: 'prev.type',
      value: 'image',
    },
    {
      name: 'relief',
      value: 'NERVAPE 3X',
    },
    {
      name: 'prev.bg',
      value: 'btcfs://0bfb23cc12eff888099b07b423632c60d2ef879a08cc02bffb9595acee2c5719i0',
    },
    {
      name: 'Color',
      value: 'Thai Special',
    },
    {
      name: 'Serial',
      value: '3X0125CLNVAP000203',
    },
    {
      name: 'Note',
      value: '',
    },
  ],
};
