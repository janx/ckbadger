import type {
  ActivitySummary24h,
  CkbytesAllocation,
  ChartResponse,
  DailyActivityStats,
  DaoStatistics,
  GlobalActivity,
  TxStatsResponse,
  UncleRateChartResponse,
} from '@/lib/api';

/**
 * The homepage's data, shaped like the live API (mainnet, 2026-10-24). Daily charts end on the
 * last COMPLETE day, 2026-10-23; tx-stats ends on the in-progress hour / day.
 */

/** `n` consecutive ISO days ending on 2026-10-23. */
export function isoDays(n: number, last = '2026-10-23'): string[] {
  const end = Date.parse(`${last}T00:00:00Z`);
  return Array.from({ length: n }, (_, i) =>
    new Date(end - (n - 1 - i) * 86_400_000).toISOString().slice(0, 10)
  );
}

function chart(title: string, values: string[], yAxisLabel: string): ChartResponse {
  const days = isoDays(values.length);
  return { title, yAxisLabel, data: values.map((value, i) => ({ date: days[i], value })) };
}

export const BLOCK_TIMES = [
  '10.02',
  '9.49',
  '9.64',
  '10.02',
  '10.10',
  '11.10',
  '10.37',
  '11.04',
  '10.07',
  '9.60',
  '8.66',
  '10.22',
  '8.93',
  '9.57',
];
export const averageBlockTimeChart = chart('Average Block Time', BLOCK_TIMES, 'Seconds');

/** H/s (the unit BE-1 pins). The last day is 91.75 PH/s. */
export const HASH_RATES = [
  '95.1',
  '94.0',
  '95.6',
  '103.1',
  '100.0',
  '102.2',
  '101.4',
  '102.2',
  '95.3',
  '93.8',
  '94.8',
  '94.5',
  '94.3',
  '91.75',
].map((ph) => String(Math.round(Number(ph) * 1e15)));
export const hashRateChart = chart('Hash Rate', HASH_RATES, 'Hash Rate');

export const UNCLE_RATES = [
  '0.0276',
  '0.0268',
  '0.0261',
  '0.0284',
  '0.0268',
  '0.0260',
  '0.0278',
  '0.0261',
  '0.0277',
  '0.0254',
  '0.0265',
  '0.0256',
  '0.0249',
  '0.026371',
];
export const uncleRateChart: UncleRateChartResponse = {
  ...chart('Uncle Rate', UNCLE_RATES, 'Uncle Rate'),
  orphanRateTarget: { numerator: 1, denominator: 40 },
};

/** Knowledge size in whole CKB: +0.2–0.4M a day, one −8.94M day (drawn clipped). */
export const KNOWLEDGE_SIZES = (() => {
  const deltas = [
    250_000, 300_000, 1_510_000, -8_940_000, 400_000, 350_000, 380_000, 300_000, 320_000, 290_000,
    310_000, 340_000, 280_000, 330_000, 300_000, 360_000, 310_000, 300_000, 260_000, 290_000,
    300_000, 310_000, 270_000, 280_000, 300_000, 320_000, 290_000, 150_000, 120_000, 82_991,
  ];
  let total = 150_000_000;
  const out = [String(total)];
  for (const d of deltas) {
    total += d;
    out.push(String(total));
  }
  return out;
})();
export const knowledgeSizeChart = chart('Common Knowledge Size', KNOWLEDGE_SIZES, 'CKB');

/** Total DAO deposit as CKB decimals; the last change is +168,366.47983386 CKB. */
export const TOTAL_DEPOSITS = (() => {
  const deltasShannons = [
    -1_000_000, -900_000, 300_000, 120_000, -11_900_000, 5_200_000, -80_000, 2_000_000, 1_500_000,
    -600_000, 28_120_000, -2_000_000, 900_000, -500_000, 1_000_000, -300_000, -1_200_000, 600_000,
    200_000, -700_000, 400_000, -900_000, 300_000, 1_100_000, -400_000, 700_000, -1_500_000,
    900_000, 1_000_000,
  ].map((ckb) => BigInt(ckb) * BigInt(100_000_000));
  deltasShannons.push(BigInt('16836647983386'));
  let total = BigInt('810000000000000000');
  const fmt = (s: bigint) => `${s / BigInt(1e8)}.${(s % BigInt(1e8)).toString().padStart(8, '0')}`;
  const out = [fmt(total)];
  for (const d of deltasShannons) {
    total += d;
    out.push(fmt(total));
  }
  return out;
})();
export const totalDepositChart = chart('Total Deposit', TOTAL_DEPOSITS, 'CKB');

const HOURS = Array.from({ length: 24 }, (_, i) => `${String((12 + i) % 24).padStart(2, '0')}:00`);
const HOURLY = [
  610, 560, 548, 590, 640, 600, 655, 645, 650, 652, 670, 612, 562, 616, 630, 760, 755, 790, 700,
  690, 720, 650, 731, 326,
];
const DAILY_LABELS = Array.from({ length: 14 }, (_, i) => `10/${String(11 + i).padStart(2, '0')}`);
const DAILY = [
  16547, 16734, 16100, 15950, 15300, 15800, 15350, 16000, 16300, 17013, 15900, 17503, 15710, 14122,
];
export const txStats: TxStatsResponse = {
  currentHour: 326,
  currentDay: 14122,
  // The newest bucket of each series is in progress; every earlier one is complete.
  hourlyData: HOURS.map((label, i) => ({
    label,
    value: HOURLY[i],
    complete: i < HOURS.length - 1,
  })),
  dailyData: DAILY_LABELS.map((label, i) => ({
    label,
    value: DAILY[i],
    complete: i < DAILY_LABELS.length - 1,
  })),
};

export const daoStatistics: DaoStatistics = {
  tipBlockNumber: 20835366,
  // contracts W5-3: the block the singleton row was computed at.
  anchor: {
    blockNumber: 20835366,
    blockHash: '0x6d0e35a3c5b4f0a27e1c9d8b7a6f5e4d3c2b1a09f8e7d6c5b4a3928170615243',
  },
  genesisBaselineBurnt: '840000000000000000',
  totalDeposited: '812889561643799700',
  totalDepositedCkb: '8128895616.437997',
  totalDepositors: 16673,
  activeDeposits: 22379,
  depositCellsCount: 20689,
  withdrawalRequestCellsCount: 1690,
  pendingWithdrawalCapacity: '90872260089687313',
  pendingWithdrawalCapacityCkb: '908722600.89687313',
  daoCapacity: '903761821733487013',
  daoFreeCapacity: '903000000000000000',
  depositorAllocationWeight: '880000000000000000',
  tipTotalIssuance: '4000000000000000000',
  tipProtocolOccupiedCapacity: '1200000000000000000',
  rolling24hBaselineBlockNumber: 20830000,
  rolling24hCutoffChainTimeMs: '1792972800000',
  totalCompensationPaid: '76079761235165103',
  totalCompensationPaidCkb: '760797612.35165103',
  unclaimedCompensation: '83759101156488264',
  unclaimedCompensationCkb: '837591011.56488264',
  averageDepositDays: '1.3K days+',
  estimatedApc: '1.99',
  miningReward: '96304962619313853',
  miningRewardCkb: '963049626.19313853',
  depositCompensation: '159838862391653367',
  depositCompensationCkb: '1598388623.91653367',
  burnt: '674731136445379763',
  burntCkb: '6747311364.45379763',
  depositChange24h: '-2578825.48689593',
  depositorsChange24h: -3,
  claimedCompensationChange24h: '0',
  unclaimedCompensationChange24h: '0',
  // ---- F2 group dao: lane R2b-a 41/44 ----
  deposits24h: 12,
  withdrawalRequests24h: 9,
  // = unclaimedCompensation + burnt, the singleton's own S
  tipSecondaryPool: '758490237601868027',
};

const SCRIPT_COUNTS = [
  {
    codeHash: '0x3a468d53352eb855521dabed0dc7036929bfe72766ad58f801edfbae564f7b43',
    name: '.bit Time Index State',
    count: 5480,
  },
  {
    codeHash: '0x9e537bf5b8ec044ca3f53355e879f3fd8832217e4a9b41d9994cf0c547241a79',
    name: '.bit Time Info',
    count: 5480,
  },
  {
    codeHash: '0xd483925160e4232b2cb29f012e8380b7b612d71cf4e79991476b6bcf610735f6',
    name: 'Bitcoin SPV Type Lock',
    count: 294,
  },
  {
    codeHash: '0x1106d9eaccde0995a7e07e80dd0ce7509f21752538dfdd1ee2526d24574846b1',
    name: '.bit Lock',
    count: 23,
  },
  {
    codeHash: '0x7a2a3e8d6e5c4a2b4f2d5c0b3b8a1e2f1c9d4e7f6a5b4c3d2e1f0a9b8c7d6e5f',
    name: '.bit Income Cell',
    count: 12,
  },
  {
    codeHash: '0x0b36e9860000000000000000000000000000000000000000000000000000beef',
    name: null,
    count: 6,
  },
  {
    codeHash: '0xe2b842a50000000000000000000000000000000000000000000000000000cafe',
    name: null,
    count: 3,
  },
  {
    codeHash: '0x5dd229e20000000000000000000000000000000000000000000000000000f00d',
    name: null,
    count: 2,
  },
];

export const activitySummary24h: ActivitySummary24h = {
  activityCount: 7115,
  transferCount: 1302,
  daoDepositCount: 4,
  daoWithdrawRequestCount: 3,
  daoWithdrawCompleteCount: 1,
  tokenCount: 0,
  objectCount: 0,
  identityCount: 10,
  scriptCallCount: 5795,
  unknownCount: 0,
  coinbaseCount: 9800,
  uniqueAddressCount: 1497,
  totalCkbMoved: '34628000000000000',
  scriptCounts: SCRIPT_COUNTS,
  hoursCovered: 24,
};

/** 29 complete days, 09/25 … 10/23, oldest first; the last day has 6,691 activities. */
export const dailyActivities: DailyActivityStats[] = isoDays(29).map((iso, i) => {
  const total = i === 28 ? 6691 : 6200 + ((i * 397) % 1500);
  return {
    date: iso.replace(/-/g, ''),
    transferCount: total - 5000,
    daoDepositCount: 0,
    daoWithdrawRequestCount: 0,
    daoWithdrawCompleteCount: 0,
    tokenCount: 0,
    objectCount: 0,
    identityCount: 0,
    scriptCallCount: 5000,
    unknownCount: 3,
    coinbaseCount: 9800,
    uniqueAddressCount: 500,
    totalCkbMoved: '0',
    scriptCounts: [],
  };
});

/** Exact complete daily transaction counts, including 9,800 cellbases each day. */
export const dailyTransactionCountChart: ChartResponse = chart(
  'Transaction Count',
  isoDays(29).map((_, i) => String(9800 + (i === 28 ? 6691 : 6200 + ((i * 397) % 1500)))),
  'Transactions'
);

const ADDRS = [
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfkcv576ccddnn4quf2ga65xee2m26h7nq3q7ue',
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq2qf8keemy2p5uu0g0gn8cd4ju23s5269qsv05e',
];

/** Nine latest activities: .bit type calls, a DAO deposit and a plain transfer. */
export const latestActivities: GlobalActivity[] = Array.from({ length: 9 }, (_, i) => {
  const base: GlobalActivity = {
    txHash: `0x${String(i + 1).padStart(2, '0')}${'ab'.repeat(30)}${String(i).padStart(2, '0')}`,
    blockNumber: 20_819_142 - i,
    txIndex: 1,
    timestamp: String(Date.now() - (i + 1) * 9000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [
      {
        typeCodeHash: SCRIPT_COUNTS[0].codeHash,
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: `0x1${i}`,
        scriptName: '.bit Time Index State',
      },
      {
        typeCodeHash: SCRIPT_COUNTS[1].codeHash,
        typeHashType: 'type',
        typeArgs: '0x00',
        scriptHash: `0x2${i}`,
        scriptName: '.bit Time Info',
      },
    ],
    lockCalls: [],
    participants: [
      {
        address: ADDRS[i % 2],
        lockHash: `0x${'cd'.repeat(32)}`,
        lockHashPrefix: null,
        roles: [],
        ckbDelta: '-30000',
        usedDelta: '0',
        itemDeltas: [],
        tags: 0,
      },
    ],
  };
  if (i === 1) {
    return {
      ...base,
      typeCalls: [],
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'deposit',
          metadata: { capacity: '10200000000' },
        },
      ],
      participants: [
        { ...base.participants[0], ckbDelta: '-10200001000' },
        { ...base.participants[0], address: ADDRS[1], ckbDelta: '0' },
      ],
    };
  }
  if (i === 2) return { ...base, typeCalls: [] };
  return base;
});

/** A party of a feed sample: a full address, deltas in shannons. */
function party(
  tail: string,
  ckbDelta: string,
  usedDelta: string,
  itemDeltas: GlobalActivity['participants'][number]['itemDeltas'] = []
): GlobalActivity['participants'][number] {
  return {
    address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${tail}`,
    lockHash: `0x${tail.padStart(64, '0')}`,
    lockHashPrefix: null,
    roles: [],
    ckbDelta,
    usedDelta,
    itemDeltas,
    tags: itemDeltas.length > 0 ? 2 : 0,
  };
}

const CKB = (whole: string) => `${whole}00000000`;

function sample(
  n: number,
  overrides: Partial<GlobalActivity> & Pick<GlobalActivity, 'participants'>
): GlobalActivity {
  return {
    txHash: `0x${String(n).padStart(2, '0')}${'cd'.repeat(30)}${String(n).padStart(2, '0')}`,
    blockNumber: 20_818_733 - n,
    txIndex: 1,
    timestamp: String(Date.now() - (n + 1) * 60_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    ...overrides,
  };
}

/** Real shapes from the FE-7 mockup: DAO, Fiber, object, token, and an 8-party transaction. */
export const feedSamples = {
  daoWithdrawRequest: sample(1, {
    protocolActions: [
      {
        protocol: 'dao',
        protocolName: 'Nervos DAO',
        action: 'withdraw_request',
        metadata: { capacity: '2605000000000', depositBlock: '19695550' },
      },
    ],
    participants: [
      party('fxdtl8', '0', '0'),
      party('r4p74p', '41232486750', CKB('61')),
      party('hxw6h7', '-41232488212', `-${CKB('61')}`),
    ],
  }),
  fiberClose: sample(2, {
    protocolActions: [
      {
        protocol: 'fiber',
        protocolName: 'Fiber',
        action: 'channel_close',
        metadata: {
          channelOutpoint: `0x0d3870bb${'00'.repeat(26)}6e26:0`,
          capacity: '75000000000',
        },
      },
    ],
    participants: [
      party('hspqyz', '25000000006', CKB('61')),
      party('f24nwy', `-${CKB('750')}`, `-${CKB('61')}`),
      party('kdeakp', '49999999456', CKB('61')),
    ],
  }),
  objectTransfer: sample(3, {
    typeCalls: [
      {
        typeCodeHash: '0xckbfs',
        typeHashType: 'type',
        typeArgs: `0xcd6ecb${'00'.repeat(14)}2500`,
        scriptHash: '0xs1',
        scriptName: 'CKBFS',
      },
    ],
    lockCalls: [
      {
        lockCodeHash: '0xnervape',
        lockHashType: 'type',
        lockArgs: `0x0131da${'00'.repeat(14)}5d74`,
        scriptHash: '0xl1',
        scriptName: 'Nervape Shadow Lock',
      },
    ],
    participants: [
      party('cjk9lz', '-52800064119', `-${CKB('502')}`, [
        { kind: 'object', objectId: `0x51c75a${'00'.repeat(26)}96f2`, delta: -1 },
      ]),
      party('shp3hr', CKB('320'), CKB('320'), [
        { kind: 'object', objectId: `0x51c75a${'00'.repeat(26)}96f2`, delta: 1 },
      ]),
      party('204m4g', CKB('208'), CKB('208')),
    ],
  }),
  tokenTransfer: sample(4, {
    lockCalls: [
      {
        lockCodeHash: `0xab0ede${'00'.repeat(25)}e09f`,
        lockHashType: 'type',
        lockArgs: `0x4b0000${'00'.repeat(14)}838e`,
        scriptHash: `0x43b2924e${'00'.repeat(26)}895d`,
      },
    ],
    participants: [
      party('3fzxu7', CKB('215'), CKB('214'), [
        {
          kind: 'token',
          typeScriptHash: '0xseal',
          delta: '3658346853885',
          symbol: 'Seal',
          decimals: 8,
        },
      ]),
      party('j3pkeh', '-21500008888', `-${CKB('369')}`, [
        {
          kind: 'token',
          typeScriptHash: '0xseal',
          delta: '-3658346853885',
          symbol: 'Seal',
          decimals: 8,
        },
      ]),
    ].map((p) => ({ ...p, tags: 1 })),
  }),
  eightParties: sample(5, {
    participants: [
      party('p1', `-${CKB('100')}`, '0'),
      party('p2', CKB('900'), '0'),
      party('p3', '0', '0'),
      party('p4', `-${CKB('700')}`, '0'),
      party('p5', CKB('300'), '0'),
      party('p6', `-${CKB('500')}`, '0'),
      party('p7', CKB('200'), '0'),
      party('p8', `-${CKB('50')}`, '0'),
    ],
  }),
};

/**
 * `/statistics/ckbytes-allocation` at one block: knowledge + DAO free + free is exactly the
 * circulation; the DAO capacity (deposits and withdraw requests) is context, not a part.
 */
export const ckbytesAllocation: CkbytesAllocation = {
  blockNumber: 20_820_045,
  circulating: '4986878303450933125',
  knowledge: '15113424500000000',
  daoFree: '821824361643799700',
  free: '4149940517307133425',
  daoCapacity: '822052561643799700',
};
