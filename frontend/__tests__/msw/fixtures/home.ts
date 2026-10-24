import type {
  ActivitySummary24h,
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
  hourlyData: HOURS.map((label, i) => ({ label, value: HOURLY[i] })),
  dailyData: DAILY_LABELS.map((label, i) => ({ label, value: DAILY[i] })),
};

export const daoStatistics: DaoStatistics = {
  totalDeposited: '812889561643799700',
  totalDepositedCkb: '8128895616.437997',
  totalDepositors: 16673,
  activeDeposits: 22379,
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
        { protocol: 'dao', action: 'deposit', metadata: { capacity: '10200000000' } },
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
