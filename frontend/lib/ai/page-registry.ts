// The production renderer, route parsers, capabilities and LLM discovery all
// consume this registry. Adding a route requires an exhaustive renderer case.
import type { ParsedMarkdownPage } from '@/lib/ai/markdown-route';

export const PAGE_ROUTES = [
  { pattern: '/', kind: 'home', rawProfiles: [] },
  { pattern: '/activities', kind: 'activities_list', rawProfiles: [] },
  { pattern: '/address/{addr}', kind: 'address_detail', rawProfiles: [] },
  { pattern: '/inventory/tokens', kind: 'inventory_tokens', rawProfiles: [] },
  { pattern: '/inventory/objects', kind: 'inventory_objects', rawProfiles: [] },
  { pattern: '/inventory/identities', kind: 'inventory_identities', rawProfiles: [] },
  { pattern: '/blocks', kind: 'blocks_list', rawProfiles: [] },
  { pattern: '/blocks/{id}', kind: 'block_detail', rawProfiles: ['default'] },
  { pattern: '/cell/{outpoint}', kind: 'cell_detail', rawProfiles: ['default'] },
  { pattern: '/charts', kind: 'charts_overview', rawProfiles: [] },
  { pattern: '/charts/{slug}', kind: 'chart_detail', rawProfiles: [] },
  { pattern: '/classes/{classId}', kind: 'classes_detail', rawProfiles: [] },
  { pattern: '/clusters/{clusterId}', kind: 'clusters_detail', rawProfiles: [] },
  { pattern: '/dao', kind: 'dao_overview', rawProfiles: [] },
  { pattern: '/dao/charts', kind: 'dao_charts', rawProfiles: [] },
  { pattern: '/forks', kind: 'forks_list', rawProfiles: [] },
  { pattern: '/forks/{id}', kind: 'fork_detail', rawProfiles: [] },
  { pattern: '/hardforks', kind: 'hardforks', rawProfiles: [] },
  { pattern: '/network', kind: 'network_overview', rawProfiles: [] },
  { pattern: '/identities/{collectionId}', kind: 'identity_collection', rawProfiles: [] },
  {
    pattern: '/identities/dotbit/{identityId}',
    kind: 'dotbit_item_detail',
    rawProfiles: ['default'],
  },
  {
    pattern: '/identities/did/{identityId}',
    kind: 'did_ckb_item_detail',
    rawProfiles: ['default'],
  },
  {
    pattern: '/identities/bit-cell/{identityId}',
    kind: 'bit_cell_item_detail',
    rawProfiles: ['default'],
  },
  { pattern: '/objects', kind: 'objects_list', rawProfiles: [] },
  { pattern: '/objects/{sporeId}', kind: 'object_detail', rawProfiles: [] },
  { pattern: '/objects/mnft/{objectId}', kind: 'mnft_item_detail', rawProfiles: ['default'] },
  { pattern: '/script/{codeHash}', kind: 'script_by_code_hash', rawProfiles: [] },
  { pattern: '/scripts', kind: 'scripts_list', rawProfiles: [] },
  { pattern: '/scripts/{name}', kind: 'script_detail', rawProfiles: [] },
  { pattern: '/tokens', kind: 'tokens_list', rawProfiles: [] },
  { pattern: '/tokens/{typeHash}', kind: 'token_detail', rawProfiles: [] },
  { pattern: '/fiber/channels', kind: 'fiber_channels_list', rawProfiles: [] },
  { pattern: '/fiber/channels/{channelId}', kind: 'fiber_channel_detail', rawProfiles: [] },
  { pattern: '/transactions', kind: 'transactions_list', rawProfiles: [] },
  { pattern: '/tx/{hash}', kind: 'tx_detail', rawProfiles: ['default', 'debugger'] },
] as const satisfies readonly {
  pattern: string;
  kind: Exclude<ParsedMarkdownPage['kind'], 'unknown'>;
  rawProfiles: readonly string[];
}[];

export const RAW_ROUTE_PROFILES: Record<string, readonly string[]> = Object.fromEntries(
  PAGE_ROUTES.filter((route) => route.rawProfiles.length > 0).map((route) => [
    route.pattern,
    route.rawProfiles,
  ])
);

export function matchPageRoute(pathname: string, raw = false): ParsedMarkdownPage {
  let normalized = `/${pathname.replace(/^\/+|\/+$/g, '')}`;
  if (normalized === '/assets') normalized = '/inventory/tokens';
  const segments = normalized.split('/');
  for (const route of PAGE_ROUTES) {
    if (raw && route.rawProfiles.length === 0) continue;
    const pattern = route.pattern.split('/');
    if (pattern.length !== segments.length) continue;
    const params: Record<string, string> = {};
    let matched = true;
    for (let i = 0; i < pattern.length; i++) {
      if (pattern[i].startsWith('{') && segments[i]) {
        // Malformed percent encoding is a client error, never an alternate key.
        const value = decodeURIComponent(segments[i]);
        if (value.includes('/') || /[\\?#]/.test(value) || value === '.' || value === '..') {
          throw new URIError(`Invalid page parameter: ${segments[i]}`);
        }
        params[pattern[i].slice(1, -1)] = value;
      } else if (pattern[i] !== segments[i]) {
        matched = false;
        break;
      }
    }
    if (matched) return { kind: route.kind, pathname: normalized, ...params } as ParsedMarkdownPage;
  }
  return { kind: 'unknown', pathname: normalized };
}

export const CHART_ROUTES = {
  'address-cohort-retention': 'getAddressCohortRetentionChart',
  'average-block-time': 'getAverageBlockTimeChart',
  'block-time-distribution': 'getBlockTimeDistributionChart',
  'capacity-turnover-ratio': 'getCapacityTurnoverRatioChart',
  'cell-count': 'getCellCountChart',
  'cell-size-distribution': 'getCellSizeDistributionChart',
  'circulation-ratio': 'getDaoCirculationRatioChart',
  'common-knowledge-composition': 'getCommonKnowledgeCompositionChart',
  'daily-deposit': 'getDaoDailyDepositChart',
  difficulty: 'getDifficultyChart',
  'epoch-time-distribution': 'getEpochTimeDistributionChart',
  'epoch-time-length': 'getEpochTimeLengthChart',
  'hash-rate': 'getHashRateChart',
  'hodl-wave': 'getHodlWaveChart',
  'inflation-rate': 'getInflationRateChart',
  'knowledge-size': 'getKnowledgeSizeChart',
  'miner-address-distribution': 'getMinerAddressDistributionChart',
  'most-utilized-assets': 'getMostUtilizedAssetsChart',
  'most-utilized-scripts': 'getMostUtilizedScriptsChart',
  'nominal-apc': 'getNominalApcChart',
  'secondary-issuance': 'getSecondaryIssuanceChart',
  'total-deposit': 'getDaoTotalDepositChart',
  'total-supply': 'getTotalSupplyChart',
  'transaction-count': 'getTransactionCountChart',
  'uncle-rate': 'getUncleRateChart',
} as const;
