export interface LegendDescriptionItem {
  label: string;
  description: string;
}

/**
 * A chart page's evidence tier (lane D1, the chart-hero board, rule 10; CH-15): one sentence of
 * what is computed, one formula line in the fields the API sends the page (and the store fields
 * they come from where the route reads them by name), and the legend items only where a page's
 * were written by hand and are good (hash rate, difficulty, uncle rate, inflation, CKB moved); a
 * line that explained a series by its own name, a word swapped per series, is gone.
 */
export interface ChartDescription {
  overview: string;
  formula: string;
  legendItems: LegendDescriptionItem[];
}

interface ChartDescriptionContext {
  yAxisLabel?: string;
  y2AxisLabel?: string;
  seriesLabels?: string[];
}

type ChartDescriptionBuilder = (ctx: ChartDescriptionContext) => ChartDescription;

const CHART_DESCRIPTION_BUILDERS: Record<string, ChartDescriptionBuilder> = {
  'dao-chart-total-deposit': () => ({
    overview:
      'Shows the CKB locked in live phase-0 Nervos DAO deposit cells at each complete UTC+8 day, and the addresses that hold them.',
    formula:
      'value = total_deposited, the capacity of the live phase-0 deposit cells at the day’s end; value2 = the addresses holding at least one of them.',
    legendItems: [],
  }),
  'dao-chart-daily-deposit': () => ({
    overview: 'Shows the CKB newly deposited into the Nervos DAO on each complete UTC+8 day.',
    formula:
      'value = Σ capacity of the outputs that open a deposit in the day’s confirmed transactions; value2 = how many such outputs.',
    legendItems: [],
  }),
  'dao-chart-circulation-ratio': () => ({
    overview:
      'Shows the share of circulating CKB locked in Nervos DAO deposits at the end of each complete UTC+8 day.',
    formula:
      'value = total_deposited ÷ circulating × 100, cut at four decimals, where circulating = C − genesis burnt − S from the DAO field of the day’s last block.',
    legendItems: [],
  }),
  'chart-transaction-count': () => ({
    overview: 'Counts the transactions confirmed on each complete UTC+8 day.',
    formula: 'value = Σ, over the canonical blocks of the day, of each block’s transactions.',
    legendItems: [],
  }),
  'chart-cell-count': () => ({
    overview:
      'Counts the cells that are live at the end of each complete UTC+8 day, and the cells consumed since genesis.',
    formula:
      'liveCells = live cells at the day’s end; deadCells = cells consumed since genesis; allCells = liveCells + deadCells.',
    legendItems: [],
  }),
  'chart-knowledge-size': () => ({
    overview:
      'Shows the CKB occupied by live cells, the common knowledge, at the end of each complete UTC+8 day, its share of the capacity base and its change from day to day.',
    formula:
      'value = U (dao[24..32]) of the day’s last block − the genesis virtual occupied capacity; value2 = value ÷ the capacity base × 100; net flow = value − value of the day before.',
    legendItems: [],
  }),
  'chart-common-knowledge-composition': () => ({
    overview:
      'Splits the CKB occupied by live cells by what each cell holds, at the end of each complete UTC+8 day.',
    formula:
      'values.transfer, dao, udt, nftSpore, otherContracts = Σ occupied capacity of the live cells of each kind at the day’s end.',
    legendItems: [],
  }),
  'chart-capacity-turnover-ratio': () => ({
    overview:
      'Shows how much of the live occupied capacity was consumed and replaced, each day and over each week.',
    formula:
      'value = used_capacity_consumed ÷ knowledge_size × 100 for the day; value2 = Σ used_capacity_consumed ÷ Σ knowledge_size × 100 over the seven days ending it; both cut at six decimals.',
    legendItems: [],
  }),
  'chart-cell-size-distribution': () => ({
    overview: 'Groups the live cells by the CKB they occupy.',
    formula:
      'value = the live cells whose occupied capacity falls in the bucket; value2 = Σ occupied capacity of those cells.',
    legendItems: [],
  }),
  'chart-address-cohort-retention': () => ({
    overview:
      'Groups addresses by the month they were first seen, then shows how much of each cohort’s balance is occupied today.',
    formula:
      'value = Σ common_knowledge_size ÷ Σ balance × 100 over the addresses first seen in the month, cut at six decimals; value2 = Σ common_knowledge_size.',
    legendItems: [],
  }),
  'chart-most-utilized-scripts': () => ({
    overview:
      'Ranks scripts by the CKB their live cells occupy, across every deployment of a script.',
    formula:
      'usedShare.topN = Σ occupied capacity of the live cells a script locks or types, the 20 largest named; capacityShare.topN = Σ their full capacity; drawnTotal = Σ the five largest, which the page draws; rest = the day’s whole − drawnTotal.',
    legendItems: [],
  }),
  'chart-most-utilized-assets': () => ({
    overview: 'Ranks tokens and object collections by the CKB their live cells occupy.',
    formula:
      'usedShare.topN = Σ occupied capacity of an asset’s live cells, from exact daily deltas, the 20 largest named; capacityShare.topN = Σ their full capacity; drawnTotal = Σ the five largest; rest = the day’s whole − drawnTotal.',
    legendItems: [],
  }),
  'chart-block-time-distribution': () => ({
    overview:
      'Groups the blocks of the last 42 complete epochs, or of every complete epoch on a younger chain, by the time since their parent.',
    formula:
      'value = the blocks whose timestamp − parent timestamp falls in the 0.1 s bucket × 100 ÷ intervals, every block of the window, cut at three decimals; beyondRange = the blocks over 50 s, in intervals and in no bucket.',
    legendItems: [],
  }),
  'chart-epoch-time-distribution': () => ({
    overview: 'Groups the completed epochs by how long each took.',
    formula: 'value = the completed epochs whose duration, rounded to the minute, is the bucket’s.',
    legendItems: [],
  }),
  'chart-epoch-time-length': () => ({
    overview:
      'Shows how long each completed epoch took, its blocks, and the consensus target duration.',
    formula:
      'value = timestamp(last block) − timestamp(first block) of the epoch, in whole ms; value2 = its blocks; target = the consensus epoch duration.',
    legendItems: [],
  }),
  'chart-average-block-time': () => ({
    overview: 'Shows the average time between blocks on each complete UTC+8 day.',
    formula:
      'value = DailyStats::avg_block_time_ms: Σ (timestamp − parent timestamp) ÷ the number of the day’s canonical blocks, in whole ms, cut.',
    legendItems: [],
  }),
  'chart-hash-rate': ({ yAxisLabel }) => ({
    overview:
      "For each complete UTC+8 day: the sum of the difficulty of that day's blocks, divided by the sum of their intervals in seconds. The day in progress is left out until it is complete.",
    formula:
      'value = Σ difficulty of the day’s blocks ÷ Σ their intervals in seconds, in PH/s (10^15).',
    legendItems: [
      {
        label: yAxisLabel ?? 'Hash Rate',
        description:
          'Hashes per second, drawn in PH/s (10^15). Every day is its own estimate; nothing is averaged across days.',
      },
    ],
  }),
  'chart-difficulty': ({ yAxisLabel }) => ({
    overview:
      "For each complete UTC+8 day: the mean difficulty of that day's blocks, the sum of their header difficulties divided by the number of blocks.",
    formula: 'value = Σ header difficulty of the day’s blocks ÷ their number, in P (10^15).',
    legendItems: [
      {
        label: yAxisLabel ?? 'Difficulty',
        description: 'Drawn in P (10^15). The day in progress is left out until it is complete.',
      },
    ],
  }),
  'chart-uncle-rate': ({ yAxisLabel }) => ({
    overview:
      "For each complete UTC+8 day: the uncles included by that day's blocks divided by the number of blocks, as a percentage. The dashed line is the consensus orphan-rate target the difficulty adjustment aims for.",
    formula:
      'value = uncles included by the day’s blocks ÷ the day’s blocks, cut at six decimals, drawn × 100; target = orphanRateTarget.numerator ÷ denominator × 100.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Uncle Rate',
        description:
          'The ratio of two daily counts, cut at six decimals; the day in progress is left out.',
      },
    ],
  }),
  'chart-miner-address-distribution': () => ({
    overview: 'Ranks the miners of the last 7 complete UTC+8 days by the blocks they found.',
    formula:
      'blocksMined = the canonical blocks whose cellbase pays the miner’s lock; share = blocksMined ÷ totalBlocks of the window.',
    legendItems: [],
  }),
  'chart-total-supply': () => ({
    overview:
      'Shows where the issued CKB, C, is at the end of each complete UTC+8 day, in five parts that make it: outside the Nervos DAO, deposited in it, withdrawing from it, DAO compensation not yet claimed, and burnt. Circulating CKB, C − genesis burnt − S, is the first three.',
    formula:
      'outsideDao = C − genesis burnt − S − protocol_deposited; daoDeposited = total_deposited; daoWithdrawing = protocol_deposited − total_deposited; unclaimedCompensation = unclaimed compensation, in S; burnt = genesis burnt + rest of S; the five make C.',
    legendItems: [],
  }),
  'chart-nominal-apc': () => ({
    overview:
      'Shows a protocol-rate model by years since genesis, not a sequence of observed days.',
    formula:
      'value(year) = the year’s secondary issuance ÷ the issued supply that year, primary-issuance halvings modeled, × 100; the genesis issuance is this network’s.',
    legendItems: [],
  }),
  'chart-secondary-issuance': () => ({
    overview:
      'Shows the secondary issuance since genesis by where it went, at each complete UTC+8 day.',
    formula:
      'values.burnt, compensation, mining = cumulative secondary issuance to the treasury, to DAO depositors (claimed or not) and to miners; protocolTotalShannons = miner + S + claimed, the one whole the three make, checked per day.',
    legendItems: [],
  }),
  'chart-inflation-rate': ({ yAxisLabel }) => ({
    overview:
      'Shows realized issuance growth over each trailing 365 complete UTC+8 days from on-chain DAO headers.',
    formula:
      'value = (C(day) − C(day − 365)) ÷ C(day − 365) × 100; value2 = the same less the secondary issuance of those 365 days.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Inflation Rate',
        description:
          'Nominal uses total DAO C-field growth; real removes the exact secondary-issuance growth, leaving primary-issuance dilution. Both divide by the C field 365 days earlier.',
      },
    ],
  }),
  'chart-hodl-wave': () => ({
    overview:
      'Splits all CKB by the time since it last moved, at the end of each complete UTC+8 day.',
    formula:
      'share = band capacity × 100 ÷ Σ band capacities (DailyHodlWave.band_24h … band_gt_3y), cut at two decimals; 1 to 6 months = band_3m_6m + band_1m_3m; under 1 month = band_1w_1m + band_1d_1w + band_24h; holderCount = addresses with a positive balance.',
    legendItems: [],
  }),
  'chart-activity-volume': () => ({
    overview:
      'Counts each complete UTC+8 day’s activities under every kind they are, across transfers, DAO changes, tokens, objects, identities and script calls: an activity of two kinds counts under both, so the series is the day’s kind counts, more than its activities.',
    formula:
      'value = kindOccurrences = transferCount + daoDepositCount + daoWithdrawRequestCount + daoWithdrawCompleteCount + tokenCount + objectCount + identityCount + scriptCallCount, as the route sums them.',
    legendItems: [],
  }),
  'chart-activity-type-breakdown': () => ({
    overview:
      'Splits each day’s kind counts by kind: a band is the day’s activities of its kind, and the stack, the kinds summed, counts an activity of two kinds under both.',
    formula:
      'transfer = transferCount; dao = daoCount = daoDepositCount + daoWithdrawRequestCount + daoWithdrawCompleteCount, as the route sums them; token = tokenCount; object = objectCount; identity = identityCount; scriptCall = scriptCallCount.',
    legendItems: [],
  }),
  'chart-active-addresses': () => ({
    overview:
      'Counts the distinct addresses that took part in the indexed activities of each complete UTC+8 day.',
    formula:
      'value = uniqueAddressCount: each address once a day, whatever its number of activities.',
    legendItems: [],
  }),
  'chart-ckb-volume': () => ({
    overview:
      'Sums the exact CKB capacity moved by indexed transfer activities for each canonical chain day.',
    formula:
      'value = totalCkbMoved ÷ 10^8: the shannons moved by the day’s indexed transfer activities, in CKB.',
    legendItems: [
      {
        label: 'CKB moved',
        description:
          'The activity index records moved capacity in shannons; the chart converts it exactly to CKB.',
      },
    ],
  }),
};

export function getChartDescription(
  queryKey: string,
  context: ChartDescriptionContext = {}
): ChartDescription | undefined {
  return CHART_DESCRIPTION_BUILDERS[queryKey]?.(context);
}
