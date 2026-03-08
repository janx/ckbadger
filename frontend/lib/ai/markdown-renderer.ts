import type { ScriptRefHashType } from '@/lib/script-ref';
import {
  api,
  type Address,
  type Block,
  type Cell,
  type ChartResponse,
  type DaoDeposit,
  type GraphResponse,
  type HardforkTimelineResponse,
  type MinerDistributionResponse,
  type MostUtilizedAssetsChartResponse,
  type MostUtilizedScriptsChartResponse,
  type ReorgDetail,
  type StackedAreaChartResponse,
  type Token,
  type TokenHolder,
  type TokenTransfer,
  type Transaction,
  type TransactionDetail,
  type TransactionLifecycle,
} from '@/lib/api';
import { buildMarkdownDocument, markdownList, markdownTable } from '@/lib/ai/markdown-format';
import { CHART_PAGE_SLUGS, type ParsedMarkdownPage } from '@/lib/ai/markdown-route';
import { resolveBuildVersion } from '@/lib/runtime-config';
import { analyzeWitness, buildScriptGroupLens, inferWitnessInsights } from '@/lib/witness-analysis';

const DEFAULT_LIMIT = 20;
const MAX_LIMIT = 200;

type MarkdownChartPayload =
  | ChartResponse
  | StackedAreaChartResponse
  | MinerDistributionResponse
  | MostUtilizedScriptsChartResponse
  | MostUtilizedAssetsChartResponse;

export class MarkdownRenderError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'MarkdownRenderError';
    this.status = status;
  }
}

interface RenderMarkdownInput {
  page: ParsedMarkdownPage;
  searchParams: URLSearchParams;
  origin: string;
}

interface RenderMarkdownOutput {
  status: number;
  body: string;
}

function parseLimit(searchParams: URLSearchParams): number {
  const raw = searchParams.get('limit');
  if (raw === null) return DEFAULT_LIMIT;
  if (!/^\d+$/.test(raw)) {
    throw new MarkdownRenderError(400, `Invalid query param "limit": ${raw}`);
  }
  const limit = Number(raw);
  if (!Number.isInteger(limit) || limit < 1 || limit > MAX_LIMIT) {
    throw new MarkdownRenderError(
      400,
      `Invalid query param "limit": ${raw}. Expected integer in [1, ${MAX_LIMIT}]`
    );
  }
  return limit;
}

function parseOptionalInt(
  searchParams: URLSearchParams,
  key: string,
  minInclusive: number
): number | undefined {
  const raw = searchParams.get(key);
  if (raw === null) return undefined;
  if (!/^-?\d+$/.test(raw)) {
    throw new MarkdownRenderError(400, `Invalid query param "${key}": ${raw}`);
  }
  const value = Number(raw);
  if (!Number.isInteger(value) || value < minInclusive) {
    throw new MarkdownRenderError(
      400,
      `Invalid query param "${key}": ${raw}. Expected integer >= ${minInclusive}`
    );
  }
  return value;
}

function parsePositiveIntField(name: string, raw: string): number {
  if (!/^\d+$/.test(raw)) {
    throw new MarkdownRenderError(400, `Invalid ${name}: ${raw}`);
  }
  const value = Number(raw);
  if (!Number.isInteger(value) || value < 0) {
    throw new MarkdownRenderError(400, `Invalid ${name}: ${raw}`);
  }
  return value;
}

function buildMeta(pathname: string, pageType: string, origin: string) {
  const canonical = `${origin}${pathname}`;
  return {
    title: `ckbadger markdown - ${pathname}`,
    path: pathname,
    canonical,
    pageType,
    generatedAt: new Date().toISOString(),
    buildVersion: resolveBuildVersion(),
  };
}

function hashShort(value: string, start: number = 10, end: number = 8): string {
  if (value.length <= start + end + 3) return value;
  return `${value.slice(0, start)}...${value.slice(-end)}`;
}

function parseOutpoint(outpoint: string): { txHash: string; outputIndex: number } {
  const delimiter = outpoint.lastIndexOf('-');
  if (delimiter < 1 || delimiter === outpoint.length - 1) {
    throw new MarkdownRenderError(
      400,
      `Invalid outpoint "${outpoint}". Expected "{txHash}-{outputIndex}"`
    );
  }
  const txHash = outpoint.slice(0, delimiter);
  const outputIndex = parsePositiveIntField('outputIndex', outpoint.slice(delimiter + 1));
  return { txHash, outputIndex };
}

function parseScriptHashType(raw: string | null): ScriptRefHashType {
  if (raw === null) return 'type';
  if (raw === 'type' || raw === 'data' || raw === 'data1' || raw === 'data2') {
    return raw;
  }
  throw new MarkdownRenderError(
    400,
    `Invalid query param "hashType": ${raw}. Expected one of type,data,data1,data2`
  );
}

function parseMnftActivityAction(raw: string | null): 'mint' | 'transfer' | 'burn' | undefined {
  if (raw === null) return undefined;
  if (raw === 'mint' || raw === 'transfer' || raw === 'burn') {
    return raw;
  }
  throw new MarkdownRenderError(
    400,
    `Invalid query param "action": ${raw}. Expected one of mint,transfer,burn`
  );
}

function renderCursorMeta(searchParams: URLSearchParams) {
  return markdownTable(
    ['param', 'value'],
    [
      ['limit', parseLimit(searchParams)],
      ['cursor', searchParams.get('cursor') ?? '-'],
    ]
  );
}

function renderBlockRows(blocks: Block[]): unknown[][] {
  return blocks.map((block) => [
    block.number,
    hashShort(block.hash),
    block.transactionsCount,
    block.proposalsCount,
    block.timestamp,
  ]);
}

function renderTxRows(txs: Transaction[]): unknown[][] {
  return txs.map((tx) => [
    hashShort(tx.hash),
    tx.blockNumber,
    tx.inputsCount,
    tx.outputsCount,
    tx.fee,
    tx.timestamp,
  ]);
}

function renderChartData(chart: ChartResponse): string {
  const rows = chart.data
    .slice(0, 120)
    .map((point) => [point.date, point.value, point.value2 ?? '-']);
  return [
    `## ${chart.title}`,
    '',
    markdownTable(['date', 'value', 'value2'], rows),
    '',
    `Total points: ${chart.data.length}`,
  ].join('\n');
}

function renderStackedChartData(chart: StackedAreaChartResponse): string {
  const keys = chart.series.map((series) => series.key);
  const rows = chart.data
    .slice(0, 120)
    .map((point) => [point.date, ...keys.map((key) => point.values[key] ?? '-')]);
  return [
    `## ${chart.title}`,
    '',
    markdownTable(['date', ...keys], rows),
    '',
    `Total points: ${chart.data.length}`,
  ].join('\n');
}

function renderMostUtilizedScripts(chart: MostUtilizedScriptsChartResponse): string {
  return [
    `## ${chart.title}`,
    '',
    renderStackedChartData(chart.occupiedShare),
    '',
    renderStackedChartData(chart.capacityShare),
  ].join('\n');
}

function renderMostUtilizedAssets(chart: MostUtilizedAssetsChartResponse): string {
  return [
    `## ${chart.title}`,
    '',
    renderStackedChartData(chart.occupiedShare),
    '',
    renderStackedChartData(chart.capacityShare),
  ].join('\n');
}

function renderMinerDistribution(chart: MinerDistributionResponse): string {
  return [
    `## ${chart.title}`,
    '',
    markdownTable(
      ['address', 'minerName', 'blocksMined', 'percentage'],
      chart.data.map((miner) => [
        hashShort(miner.address),
        miner.minerName ?? '-',
        miner.blocksMined,
        miner.percentage,
      ])
    ),
    '',
    `Total blocks: ${chart.totalBlocks}`,
  ].join('\n');
}

function renderMarkdownForChart(slug: string, payload: MarkdownChartPayload): string {
  if (slug === 'most-utilized-assets') {
    return renderMostUtilizedAssets(payload as MostUtilizedAssetsChartResponse);
  }

  if (slug === 'most-utilized-scripts') {
    return renderMostUtilizedScripts(payload as MostUtilizedScriptsChartResponse);
  }

  if (slug === 'miner-address-distribution') {
    return renderMinerDistribution(payload as MinerDistributionResponse);
  }

  if ('series' in payload && 'data' in payload) {
    return renderStackedChartData(payload as StackedAreaChartResponse);
  }

  if ('yAxisLabel' in payload && 'data' in payload) {
    return renderChartData(payload as ChartResponse);
  }

  throw new MarkdownRenderError(500, `Unsupported chart payload for slug "${slug}"`);
}

function selectChartFetcher(slug: string): (() => Promise<MarkdownChartPayload>) | null {
  switch (slug) {
    case 'address-cohort-retention':
      return () => api.getAddressCohortRetentionChart();
    case 'average-block-time':
      return () => api.getAverageBlockTimeChart();
    case 'block-time-distribution':
      return () => api.getBlockTimeDistributionChart();
    case 'capacity-turnover-ratio':
      return () => api.getCapacityTurnoverRatioChart();
    case 'cell-age-vs-occupied-capacity':
      return () => api.getCellAgeVsOccupiedCapacityChart();
    case 'cell-count':
      return () => api.getCellCountChart();
    case 'cell-size-distribution':
      return () => api.getCellSizeDistributionChart();
    case 'circulation-ratio':
      return () => api.getDaoCirculationRatioChart();
    case 'common-knowledge-composition':
      return () => api.getCommonKnowledgeCompositionChart();
    case 'daily-deposit':
      return () => api.getDaoDailyDepositChart();
    case 'difficulty':
      return () => api.getDifficultyChart();
    case 'epoch-time-distribution':
      return () => api.getEpochTimeDistributionChart();
    case 'epoch-time-length':
      return () => api.getEpochTimeLengthChart();
    case 'hash-rate':
      return () => api.getHashRateChart();
    case 'hodl-wave':
      return () => api.getHodlWaveChart();
    case 'inflation-rate':
      return () => api.getInflationRateChart();
    case 'knowledge-size':
      return () => api.getKnowledgeSizeChart();
    case 'miner-address-distribution':
      return () => api.getMinerAddressDistributionChart();
    case 'most-utilized-assets':
      return () => api.getMostUtilizedAssetsChart();
    case 'most-utilized-scripts':
      return () => api.getMostUtilizedScriptsChart();
    case 'nominal-apc':
      return () => api.getNominalApcChart();
    case 'secondary-issuance':
      return () => api.getSecondaryIssuanceChart();
    case 'total-deposit':
      return () => api.getDaoTotalDepositChart();
    case 'total-supply':
      return () => api.getTotalSupplyChart();
    case 'transaction-count':
      return () => api.getTransactionCountChart();
    case 'uncle-rate':
      return () => api.getUncleRateChart();
    default:
      return null;
  }
}

function renderAddressSummary(address: Address) {
  return markdownTable(
    ['field', 'value'],
    [
      ['address', address.address ?? '-'],
      ['lockScriptHash', address.lockScriptHash],
      ['balance', address.balance],
      ['occupiedCapacity', address.occupiedCapacity],
      ['liveCellsCount', address.liveCellsCount],
      ['transactionsCount', address.transactionsCount],
    ]
  );
}

function renderCellSummary(cell: Cell) {
  return markdownTable(
    ['field', 'value'],
    [
      ['txHash', cell.txHash],
      ['outputIndex', cell.outputIndex],
      ['status', cell.status ?? '-'],
      ['capacity', cell.capacity],
      ['occupiedCapacity', cell.occupiedCapacity ?? '-'],
      ['lockScriptHash', cell.lockScriptHash],
      ['typeScriptHash', cell.typeScriptHash ?? '-'],
      ['createdAtBlock', cell.createdAtBlock],
      ['consumedAtBlock', cell.consumedAtBlock ?? '-'],
      ['consumedByTx', cell.consumedByTx ?? '-'],
    ]
  );
}

function renderForkDetail(detail: ReorgDetail): string {
  return [
    '## Fork Event',
    '',
    markdownTable(
      ['field', 'value'],
      [
        ['id', detail.event.id],
        ['eventType', detail.event.eventType],
        ['depth', detail.event.depth],
        ['forkPointNumber', detail.event.forkPointNumber],
        ['oldTipNumber', detail.event.oldTipNumber],
        ['newTipNumber', detail.event.newTipNumber],
        ['detectedAt', detail.event.detectedAt],
        ['resolvedAt', detail.event.resolvedAt ?? '-'],
      ]
    ),
    '',
    '## Orphaned Blocks',
    '',
    markdownTable(
      ['number', 'hash', 'txCount', 'timestamp'],
      detail.orphanedBlocks.map((block) => [
        block.number,
        hashShort(block.hash),
        block.transactionsCount,
        block.timestamp,
      ])
    ),
    '',
    '## Orphaned Transactions',
    '',
    markdownTable(
      ['hash', 'blockNumber', 'inputs', 'outputs', 'totalCapacity'],
      detail.orphanedTransactions.map((tx) => [
        hashShort(tx.hash),
        tx.blockNumber,
        tx.inputsCount ?? '-',
        tx.outputsCount ?? '-',
        tx.totalCapacity ?? '-',
      ])
    ),
  ].join('\n');
}

function renderTokenSummary(token: Token) {
  return markdownTable(
    ['field', 'value'],
    [
      ['typeScriptHash', token.typeScriptHash],
      ['symbol', token.symbol ?? '-'],
      ['name', token.name ?? '-'],
      ['standard', token.standard],
      ['decimals', token.decimals],
      ['totalSupply', token.totalSupply],
      ['holdersCount', token.holdersCount],
      ['transfersCount', token.transfersCount],
      ['transfers24h', token.transfers24h],
      ['published', token.published],
      ['famous', token.famous],
    ]
  );
}

function renderTokenHolders(holders: TokenHolder[]) {
  return markdownTable(
    ['lockScriptHash', 'address', 'balance'],
    holders.map((holder) => [
      hashShort(holder.lockScriptHash),
      holder.address ?? '-',
      holder.balance,
    ])
  );
}

function renderTokenTransfers(transfers: TokenTransfer[]) {
  return markdownTable(
    ['txHash', 'block', 'from', 'to', 'amount', 'mint', 'burn', 'timestamp'],
    transfers.map((transfer) => [
      hashShort(transfer.txHash),
      transfer.blockNumber,
      transfer.fromAddress ?? transfer.fromLockHash ?? '-',
      transfer.toAddress ?? transfer.toLockHash,
      transfer.amount,
      transfer.isMint,
      transfer.isBurn,
      transfer.timestamp,
    ])
  );
}

function renderTxSummary(tx: TransactionDetail, lifecycle: TransactionLifecycle | null) {
  return markdownTable(
    ['field', 'value'],
    [
      ['hash', tx.hash],
      ['blockNumber', tx.blockNumber],
      ['index', tx.index],
      ['timestamp', tx.timestamp],
      ['isCellbase', tx.isCellbase],
      ['confirmations', tx.confirmations],
      ['inputsCount', tx.inputsCount],
      ['outputsCount', tx.outputsCount],
      ['fee', tx.fee],
      ['feeRate', tx.feeRate ?? '-'],
      ['txSize', tx.txSize ?? '-'],
      ['cycles', tx.cycles ?? '-'],
      ['witnessesCount', tx.witnesses?.length ?? 0],
      ['witnessesAvailable', tx.witnessesAvailable ?? tx.witnesses?.length !== 0],
      ['lifecyclePhase', lifecycle?.phase ?? '-'],
      ['commitmentDistance', lifecycle?.commitmentDistance ?? '-'],
    ]
  );
}

function renderGraphSummary(graph: GraphResponse) {
  return markdownTable(
    ['field', 'value'],
    [
      ['nodes', graph.nodes.length],
      ['links', graph.links.length],
    ]
  );
}

function renderHardforkTimeline(data: HardforkTimelineResponse) {
  return [
    '## Network',
    '',
    markdownTable(
      ['field', 'value'],
      [
        ['network', data.network],
        ['tipEpoch', data.tipEpoch],
        ['tipBlock', data.tipBlock],
      ]
    ),
    '',
    '## Hardfork Events',
    '',
    markdownTable(
      ['id', 'name', 'status', 'activationEpoch', 'activationBlock', 'activationDate'],
      data.events.map((event) => [
        event.id,
        event.name,
        event.status,
        event.activationEpoch,
        event.activationBlock ?? '-',
        event.activationDate,
      ])
    ),
  ].join('\n');
}

function renderDaoDepositsRows(deposits: DaoDeposit[]): unknown[][] {
  return deposits.map((deposit) => [
    hashShort(deposit.txHash),
    deposit.outputIndex,
    deposit.status,
    deposit.capacity,
    deposit.depositBlockNumber,
    deposit.withdrawBlock ?? '-',
    deposit.compensation ?? '-',
  ]);
}

export async function renderMarkdownPage(
  input: RenderMarkdownInput
): Promise<RenderMarkdownOutput> {
  const { page, searchParams, origin } = input;
  if (page.kind === 'unknown') {
    const body = buildMarkdownDocument(buildMeta(page.pathname, 'unknown', origin), [
      `# Unknown Page`,
      '',
      `No markdown renderer is registered for \`${page.pathname}\`.`,
    ]);
    return { status: 404, body };
  }

  switch (page.kind) {
    case 'home': {
      const [stats, blocks, txs] = await Promise.all([
        api.getNetworkStats(),
        api.getBlocks({ limit: 10 }),
        api.getTransactions({ limit: 10 }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Home',
        '',
        '## Network Stats',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['latestBlock', stats.latestBlock],
            ['avgBlockTime', stats.avgBlockTime],
            ['hashRate', stats.hashRate],
            ['difficulty', stats.difficulty],
            ['epoch', stats.epoch],
            ['tps', stats.tps],
            ['transactionsPerMinute', stats.transactionsPerMinute],
            ['transactionsPerDay', stats.transactionsPerDay],
          ]
        ),
        '',
        '## Latest Blocks',
        '',
        markdownTable(
          ['number', 'hash', 'txs', 'proposals', 'timestamp'],
          renderBlockRows(blocks.data)
        ),
        '',
        '## Latest Transactions',
        '',
        markdownTable(
          ['hash', 'blockNumber', 'inputs', 'outputs', 'fee', 'timestamp'],
          renderTxRows(txs.data)
        ),
      ]);
      return { status: 200, body };
    }
    case 'hardforks': {
      const hardforks = await api.getHardforks();
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Hardforks',
        '',
        renderHardforkTimeline(hardforks),
      ]);
      return { status: 200, body };
    }
    case 'blocks_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const blocks = await api.getBlocks({ limit, cursor });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Blocks',
        '',
        '## Query',
        '',
        renderCursorMeta(searchParams),
        '',
        '## Results',
        '',
        markdownTable(
          ['number', 'hash', 'txs', 'proposals', 'timestamp'],
          renderBlockRows(blocks.data)
        ),
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['limit', blocks.limit],
            ['total', blocks.total ?? '-'],
            ['hasMore', blocks.hasMore],
            ['nextCursor', blocks.nextCursor ?? '-'],
          ]
        ),
      ]);
      return { status: 200, body };
    }
    case 'block_detail': {
      const [block, feeStats, proposals] = await Promise.all([
        api.getBlock(page.id),
        api.getBlockFeeStats(page.id),
        api.getBlockProposals(page.id),
      ]);
      const txs = await api.getTransactions({
        blockNumber: block.number,
        limit: parseLimit(searchParams),
        cursor: searchParams.get('cursor') ?? undefined,
      });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Block ${block.number}`,
        '',
        '## Summary',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['number', block.number],
            ['hash', block.hash],
            ['parentHash', block.parentHash],
            ['timestamp', block.timestamp],
            ['epochNumber', block.epochNumber],
            ['epochIndex', block.epochIndex],
            ['epochLength', block.epochLength],
            ['transactionsCount', block.transactionsCount],
            ['proposalsCount', block.proposalsCount],
            ['unclesCount', block.unclesCount],
            ['difficulty', block.difficulty],
            ['minerAddress', block.minerAddress ?? '-'],
            ['miningReward', block.miningReward ?? '-'],
          ]
        ),
        '',
        '## Fee Stats',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['totalSize', feeStats.totalSize],
            ['totalCycles', feeStats.totalCycles],
            ['avgFeeRate', feeStats.avgFeeRate],
            ['minFeeRate', feeStats.minFeeRate],
            ['maxFeeRate', feeStats.maxFeeRate],
          ]
        ),
        '',
        '## Proposals',
        '',
        markdownTable(
          ['proposalIndex', 'proposalId', 'committedTxHash', 'committedBlockNumber'],
          proposals.map((proposal) => [
            proposal.proposalIndex,
            proposal.proposalId,
            proposal.committedTxHash ?? '-',
            proposal.committedBlockNumber ?? '-',
          ])
        ),
        '',
        '## Transactions',
        '',
        markdownTable(
          ['hash', 'blockNumber', 'inputs', 'outputs', 'fee', 'timestamp'],
          renderTxRows(txs.data)
        ),
      ]);
      return { status: 200, body };
    }
    case 'transactions_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const txs = await api.getTransactions({ limit, cursor });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Transactions',
        '',
        '## Query',
        '',
        renderCursorMeta(searchParams),
        '',
        '## Results',
        '',
        markdownTable(
          ['hash', 'blockNumber', 'inputs', 'outputs', 'fee', 'timestamp'],
          renderTxRows(txs.data)
        ),
      ]);
      return { status: 200, body };
    }
    case 'tx_detail': {
      const tx = await api.getTransactionDetail(page.hash);
      const [lifecycle, cellDeps] = await Promise.all([
        tx.isCellbase ? Promise.resolve(null) : api.getTransactionLifecycle(page.hash),
        api.getTransactionCellDeps(page.hash),
      ]);
      const witnesses = tx.witnesses ?? [];
      const witnessAnalyses = witnesses.map((witness, index) =>
        analyzeWitness(witness, index, tx.inputsCount)
      );
      const witnessRows = witnessAnalyses.map((analysis) => {
        return [
          analysis.index,
          analysis.role,
          analysis.byteLength,
          analysis.deterministic?.kind ?? '-',
          analysis.heuristicGuesses.map((guess) => guess.kind).join(', ') || '-',
        ];
      });
      const witnessInferences = inferWitnessInsights(
        tx,
        witnessAnalyses,
        buildScriptGroupLens(tx)
      ).map((inference) => [
        inference.severity,
        inference.kind,
        inference.message,
        inference.detail ?? '-',
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Transaction ${hashShort(tx.hash, 12, 12)}`,
        '',
        '## Summary',
        '',
        renderTxSummary(tx, lifecycle),
        '',
        '## Inputs',
        '',
        markdownTable(
          ['index', 'capacity', 'address', 'lockCodeHash', 'typeCodeHash'],
          (tx.inputs ?? []).map((input, index) => [
            index,
            input.capacity ?? '-',
            input.address ?? '-',
            input.lock?.codeHash ?? '-',
            input.type?.codeHash ?? '-',
          ])
        ),
        '',
        '## Outputs',
        '',
        markdownTable(
          ['index', 'capacity', 'occupiedCapacity', 'address', 'cellType'],
          (tx.outputs ?? []).map((output, index) => [
            index,
            output.capacity,
            output.occupiedCapacity,
            output.address ?? '-',
            output.cellType ?? '-',
          ])
        ),
        '',
        '## Witnesses',
        '',
        tx.witnessesAvailable === false
          ? 'Witness bytes unavailable (set `[ckb].data_path` in `ckbadger.toml` or verify RPC connectivity).'
          : witnessRows.length === 0
            ? 'No witness entries.'
            : markdownTable(
                ['index', 'role', 'bytes', 'deterministicKind', 'heuristics'],
                witnessRows
              ),
        '',
        '## Witness Inference',
        '',
        witnessInferences.length === 0
          ? 'No witness inference generated.'
          : markdownTable(['severity', 'kind', 'message', 'detail'], witnessInferences),
        '',
        '## Cell Deps',
        '',
        markdownTable(
          ['outPointTxHash', 'outPointIndex', 'depType'],
          cellDeps.map((dep) => [hashShort(dep.outPointTxHash), dep.outPointIndex, dep.depType])
        ),
      ]);
      return { status: 200, body };
    }
    case 'address_detail': {
      const address = await api.getAddress(page.addr);
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const [txs, tokens] = await Promise.all([
        api.getAddressTransactions(address.lockScriptHash, { limit, cursor }),
        api.getAddressTokens(address.lockScriptHash, { limit, cursor }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Address ${address.address ?? hashShort(address.lockScriptHash)}`,
        '',
        '## Summary',
        '',
        renderAddressSummary(address),
        '',
        '## Transactions',
        '',
        markdownTable(
          ['txHash', 'blockNumber', 'txType', 'capacityChange', 'timestamp'],
          txs.data.map((tx) => [
            hashShort(tx.txHash),
            tx.blockNumber,
            tx.txType,
            tx.capacityChange,
            tx.timestamp,
          ])
        ),
        '',
        '## Tokens',
        '',
        markdownTable(
          ['typeScriptHash', 'standard', 'name', 'symbol', 'balance'],
          tokens.data.map((token) => [
            hashShort(token.typeScriptHash),
            token.standard,
            token.name ?? '-',
            token.symbol ?? '-',
            token.balance,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'cell_detail': {
      const { txHash, outputIndex } = parseOutpoint(page.outpoint);
      const [cell, graph] = await Promise.all([
        api.getCell(txHash, outputIndex),
        api.getCellGraph(txHash, outputIndex, 2),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Cell ${hashShort(cell.txHash)}:${cell.outputIndex}`,
        '',
        '## Summary',
        '',
        renderCellSummary(cell),
        '',
        '## Graph',
        '',
        renderGraphSummary(graph),
      ]);
      return { status: 200, body };
    }
    case 'tokens_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const tokens = await api.getTokens({ limit, cursor });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Tokens',
        '',
        '## Query',
        '',
        renderCursorMeta(searchParams),
        '',
        '## Results',
        '',
        markdownTable(
          ['typeScriptHash', 'symbol', 'name', 'standard', 'holders', 'transfers24h'],
          tokens.data.map((token) => [
            hashShort(token.typeScriptHash),
            token.symbol ?? '-',
            token.name ?? '-',
            token.standard,
            token.holdersCount,
            token.transfers24h,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'token_detail': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const [token, holders, transfers] = await Promise.all([
        api.getToken(page.typeHash),
        api.getTokenHolders(page.typeHash, { limit, cursor }),
        api.getTokenTransfers(page.typeHash, { limit, cursor }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Token ${token.symbol ?? hashShort(token.typeScriptHash)}`,
        '',
        '## Summary',
        '',
        renderTokenSummary(token),
        '',
        '## Holders',
        '',
        renderTokenHolders(holders.data),
        '',
        '## Transfers',
        '',
        renderTokenTransfers(transfers.data),
      ]);
      return { status: 200, body };
    }
    case 'assets_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const assets = await api.getAssets({ limit, cursor });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Assets',
        '',
        '## Query',
        '',
        renderCursorMeta(searchParams),
        '',
        '## Results',
        '',
        markdownTable(
          ['id', 'assetType', 'standard', 'name', 'symbol', 'holders', 'transfers24h'],
          assets.data.map((asset) => [
            hashShort(asset.id),
            asset.assetType,
            asset.standard,
            asset.name ?? '-',
            asset.symbol ?? '-',
            asset.holdersCount,
            asset.transfers24h,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'nfts_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const nfts = await api.getSporeNfts({ limit, cursor });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# NFTs',
        '',
        '## Query',
        '',
        renderCursorMeta(searchParams),
        '',
        '## Results',
        '',
        markdownTable(
          ['sporeId', 'clusterId', 'owner', 'isLive', 'createdAtBlock'],
          nfts.data.map((nft) => [
            hashShort(nft.sporeId),
            nft.clusterId ?? '-',
            nft.ownerAddress ?? hashShort(nft.ownerLockHash),
            nft.isLive,
            nft.createdAtBlock,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'nft_detail': {
      const nft = await api.getSporeNft(page.sporeId);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# NFT ${hashShort(nft.sporeId, 14, 12)}`,
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['sporeId', nft.sporeId],
            ['txHash', nft.txHash],
            ['outputIndex', nft.outputIndex],
            ['clusterId', nft.clusterId ?? '-'],
            ['contentType', nft.contentType],
            ['contentSize', nft.contentSize],
            ['ownerLockHash', nft.ownerLockHash],
            ['ownerAddress', nft.ownerAddress ?? '-'],
            ['isLive', nft.isLive],
            ['createdAtBlock', nft.createdAtBlock],
            ['liveCapacity', nft.liveCapacity ?? '-'],
            ['liveOccupiedCapacity', nft.liveOccupiedCapacity ?? '-'],
          ]
        ),
      ]);
      return { status: 200, body };
    }
    case 'mnft_item_detail': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const action = parseMnftActivityAction(searchParams.get('action'));
      const [item, activities] = await Promise.all([
        api.getMnftItemDetail(page.nftId),
        api.getMnftItemActivities(page.nftId, { limit, cursor, action }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# mNFT ${hashShort(item.nftId, 14, 12)}`,
        '',
        '## Token',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['nftId', item.nftId],
            ['standard', item.standard],
            ['isLive', item.isLive],
            ['ownerLockHash', item.ownerLockHash ?? '-'],
            ['createdAtBlock', item.createdAtBlock],
            ['tokenIndex', item.tokenIndex],
            ['characteristicHex', item.characteristicHex],
            ['configure', item.configure],
            ['state', item.state],
            ['txHash', item.txHash ?? '-'],
            ['outputIndex', item.outputIndex ?? '-'],
          ]
        ),
        '',
        '## Class',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['classId', item.class.classId],
            ['issuerId', item.class.issuerId],
            ['name', item.class.name ?? '-'],
            ['description', item.class.description ?? '-'],
            ['renderer', item.class.renderer ?? '-'],
            ['total', item.class.total],
            ['issued', item.class.issued],
            ['configure', item.class.configure],
          ]
        ),
        '',
        '## Issuer',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['issuerId', item.issuer.issuerId],
            ['name', item.issuer.name ?? '-'],
            ['classCount', item.issuer.classCount],
            ['setCount', item.issuer.setCount],
            ['infoHex', item.issuer.infoHex ?? '-'],
          ]
        ),
        '',
        '## Lifecycle',
        '',
        markdownTable(
          ['event', 'blockNumber', 'txHash', 'outputIndex', 'note'],
          item.lifecycle.map((event) => [
            event.event,
            event.blockNumber ?? '-',
            event.txHash ?? '-',
            event.outputIndex ?? '-',
            event.note ?? '-',
          ])
        ),
        '',
        '## Activities',
        '',
        markdownTable(
          ['txHash', 'blockNumber', 'txIndex', 'timestamp', 'actions'],
          activities.data.map((activity) => [
            hashShort(activity.txHash),
            activity.blockNumber,
            activity.txIndex,
            activity.timestamp,
            activity.actions.join(','),
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'dotbit_item_detail': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const action = parseMnftActivityAction(searchParams.get('action'));
      const [item, activities] = await Promise.all([
        api.getDotbitItemDetail(page.nftId),
        api.getDotbitItemActivities(page.nftId, { limit, cursor, action }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# .bit ${item.name ?? hashShort(item.nftId, 14, 12)}`,
        '',
        '## Account',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['nftId', item.nftId],
            ['name', item.name ?? '-'],
            ['standard', item.standard],
            ['isLive', item.isLive],
            ['ownerLockHash', item.ownerLockHash ?? '-'],
            ['createdAtBlock', item.createdAtBlock],
            ['expiredAt', item.expiredAt ?? '-'],
            ['txHash', item.txHash ?? '-'],
            ['outputIndex', item.outputIndex ?? '-'],
          ]
        ),
        '',
        '## Activities',
        '',
        markdownTable(
          ['txHash', 'blockNumber', 'txIndex', 'timestamp', 'actions'],
          activities.data.map((activity) => [
            hashShort(activity.txHash),
            activity.blockNumber,
            activity.txIndex,
            activity.timestamp,
            activity.actions.join(','),
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'did_ckb_item_detail': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const action = parseMnftActivityAction(searchParams.get('action'));
      const [item, activities] = await Promise.all([
        api.getDidCkbItemDetail(page.nftId),
        api.getDidCkbItemActivities(page.nftId, { limit, cursor, action }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# did:ckb ${item.name ?? hashShort(item.nftId, 14, 12)}`,
        '',
        '## Identity',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['nftId', item.nftId],
            ['name', item.name ?? '-'],
            ['standard', item.standard],
            ['isLive', item.isLive],
            ['ownerLockHash', item.ownerLockHash ?? '-'],
            ['createdAtBlock', item.createdAtBlock],
            ['txHash', item.txHash ?? '-'],
            ['outputIndex', item.outputIndex ?? '-'],
          ]
        ),
        '',
        '## Activities',
        '',
        markdownTable(
          ['txHash', 'blockNumber', 'txIndex', 'timestamp', 'actions'],
          activities.data.map((activity) => [
            hashShort(activity.txHash),
            activity.blockNumber,
            activity.txIndex,
            activity.timestamp,
            activity.actions.join(','),
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'clusters_detail': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const [cluster, spores] = await Promise.all([
        api.getSporeCluster(page.clusterId),
        api.getSporesByCluster(page.clusterId, { limit, cursor }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Cluster ${cluster.name ?? hashShort(cluster.clusterId)}`,
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['clusterId', cluster.clusterId],
            ['name', cluster.name ?? '-'],
            ['description', cluster.description ?? '-'],
            ['ownerLockHash', cluster.ownerLockHash],
            ['ownerAddress', cluster.ownerAddress ?? '-'],
            ['sporesCount', cluster.sporesCount],
            ['createdAtBlock', cluster.createdAtBlock],
            ['liveCapacity', cluster.liveCapacity ?? '-'],
            ['liveOccupiedCapacity', cluster.liveOccupiedCapacity ?? '-'],
          ]
        ),
        '',
        '## Spores',
        '',
        markdownTable(
          ['sporeId', 'owner', 'isLive', 'createdAtBlock'],
          spores.data.map((nft) => [
            hashShort(nft.sporeId),
            nft.ownerAddress ?? hashShort(nft.ownerLockHash),
            nft.isLive,
            nft.createdAtBlock,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'scripts_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const scripts = await api.getScripts({ limit, cursor });
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Scripts',
        '',
        '## Query',
        '',
        renderCursorMeta(searchParams),
        '',
        '## Results',
        '',
        markdownTable(
          ['name', 'codeHash', 'hashType', 'scriptKind', 'network', 'deprecated'],
          scripts.data.map((script) => [
            script.name,
            hashShort(script.codeHash),
            script.hashType ?? '-',
            script.scriptKind ?? '-',
            script.network,
            script.deprecated,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'script_detail': {
      const [scripts, usage] = await Promise.all([
        api.getScript(page.name),
        api.getScriptUsage(page.name),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Script ${page.name}`,
        '',
        '## Usage',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['name', usage.name],
            ['cellsCount', usage.cellsCount],
            ['liveCellsCount', usage.liveCellsCount],
            ['capacitySum', usage.capacitySum],
            ['liveCapacitySum', usage.liveCapacitySum],
            ['occupiedCapacitySum', usage.occupiedCapacitySum],
            ['liveOccupiedCapacitySum', usage.liveOccupiedCapacitySum],
          ]
        ),
        '',
        '## Deployments',
        '',
        markdownTable(
          ['codeHash', 'scriptKind', 'cellsCount', 'liveCellsCount', 'liveOccupiedCapacitySum'],
          usage.byDeployment.map((deployment) => [
            hashShort(deployment.codeHash),
            deployment.scriptKind ?? '-',
            deployment.cellsCount,
            deployment.liveCellsCount,
            deployment.liveOccupiedCapacitySum,
          ])
        ),
        '',
        '## Registry Entries',
        '',
        markdownTable(
          ['name', 'codeHash', 'hashType', 'scriptKind', 'deprecated', 'isSystem'],
          scripts.map((script) => [
            script.name,
            hashShort(script.codeHash),
            script.hashType ?? '-',
            script.scriptKind ?? '-',
            script.deprecated,
            script.isSystem,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'script_by_code_hash': {
      const hashType = parseScriptHashType(searchParams.get('hashType'));
      const [lookup, codeCell] = await Promise.all([
        api.lookupScripts([page.codeHash]),
        api.getCodeCell(page.codeHash, hashType),
      ]);
      const matched = lookup[page.codeHash];
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Script Code Hash ${hashShort(page.codeHash, 14, 12)}`,
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['codeHash', page.codeHash],
            ['queryHashType', hashType],
            ['knownName', matched?.name ?? '-'],
            ['knownScriptKind', matched?.scriptKind ?? '-'],
            ['knownHashType', matched?.hashType ?? '-'],
            ['knownLiveCellsCount', matched?.liveCellsCount ?? '-'],
            ['knownLiveCapacitySum', matched?.liveCapacitySum ?? '-'],
            ['knownLiveOccupiedCapacitySum', matched?.liveOccupiedCapacitySum ?? '-'],
            ['codeCellTxHash', codeCell.txHash],
            ['codeCellOutputIndex', codeCell.outputIndex],
          ]
        ),
      ]);
      return { status: 200, body };
    }
    case 'dao_overview': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const status = parseOptionalInt(searchParams, 'status', 0);
      const [stats, deposits] = await Promise.all([
        api.getDaoStatistics(),
        api.getDaoDeposits({ limit, cursor, status }),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# DAO',
        '',
        '## Statistics',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['totalDeposited', stats.totalDeposited],
            ['totalDepositedCkb', stats.totalDepositedCkb],
            ['totalDepositors', stats.totalDepositors],
            ['activeDeposits', stats.activeDeposits],
            ['totalCompensationPaid', stats.totalCompensationPaid],
            ['totalCompensationPaidCkb', stats.totalCompensationPaidCkb],
            ['unclaimedCompensation', stats.unclaimedCompensation],
            ['unclaimedCompensationCkb', stats.unclaimedCompensationCkb],
            ['averageDepositDays', stats.averageDepositDays],
            ['estimatedApc', stats.estimatedApc],
          ]
        ),
        '',
        '## Deposits',
        '',
        markdownTable(
          [
            'txHash',
            'outputIndex',
            'status',
            'capacity',
            'depositBlock',
            'withdrawBlock',
            'compensation',
          ],
          renderDaoDepositsRows(deposits.data)
        ),
      ]);
      return { status: 200, body };
    }
    case 'dao_charts': {
      const [totalDeposit, dailyDeposit, circulationRatio] = await Promise.all([
        api.getDaoTotalDepositChart(),
        api.getDaoDailyDepositChart(),
        api.getDaoCirculationRatioChart(),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# DAO Charts',
        '',
        renderChartData(totalDeposit),
        '',
        renderChartData(dailyDeposit),
        '',
        renderChartData(circulationRatio),
      ]);
      return { status: 200, body };
    }
    case 'forks_list': {
      const limit = parseLimit(searchParams);
      const cursor = searchParams.get('cursor') ?? undefined;
      const [forks, recent] = await Promise.all([
        api.getForks({ limit, cursor }),
        api.getRecentReorg(),
      ]);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Forks',
        '',
        '## Recent Reorg',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['hasRecentReorg', recent.hasRecentReorg],
            ['reorgId', recent.reorg?.id ?? '-'],
            ['reorgDepth', recent.reorg?.depth ?? '-'],
            ['deepForkDetected', recent.deepFork.detected],
            ['deepForkDepth', recent.deepFork.depth ?? '-'],
          ]
        ),
        '',
        '## Events',
        '',
        markdownTable(
          ['id', 'eventType', 'depth', 'forkPointNumber', 'oldTip', 'newTip', 'detectedAt'],
          forks.data.map((fork) => [
            fork.id,
            fork.eventType,
            fork.depth,
            fork.forkPointNumber,
            fork.oldTipNumber,
            fork.newTipNumber,
            fork.detectedAt,
          ])
        ),
      ]);
      return { status: 200, body };
    }
    case 'fork_detail': {
      const id = parsePositiveIntField('fork id', page.id);
      const detail = await api.getForkDetail(id);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Fork ${id}`,
        '',
        renderForkDetail(detail),
      ]);
      return { status: 200, body };
    }
    case 'charts_overview': {
      const stats = await api.getNetworkStats();
      const chartItems = CHART_PAGE_SLUGS.map((slug) => `/charts/${slug}.md`);
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        '# Charts',
        '',
        '## Network Snapshot',
        '',
        markdownTable(
          ['field', 'value'],
          [
            ['latestBlock', stats.latestBlock],
            ['avgBlockTime', stats.avgBlockTime],
            ['hashRate', stats.hashRate],
            ['difficulty', stats.difficulty],
            ['epoch', stats.epoch],
          ]
        ),
        '',
        '## Chart Markdown Endpoints',
        '',
        markdownList(chartItems),
      ]);
      return { status: 200, body };
    }
    case 'chart_detail': {
      const fetcher = selectChartFetcher(page.slug);
      if (!fetcher) {
        throw new MarkdownRenderError(
          404,
          `Unknown chart slug "${page.slug}". Known slugs: ${CHART_PAGE_SLUGS.join(', ')}`
        );
      }
      const payload = await fetcher();
      const body = buildMarkdownDocument(buildMeta(page.pathname, page.kind, origin), [
        `# Chart ${page.slug}`,
        '',
        renderMarkdownForChart(page.slug, payload),
      ]);
      return { status: 200, body };
    }
  }
}
