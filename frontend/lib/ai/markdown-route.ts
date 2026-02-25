export const CHART_PAGE_SLUGS = [
  'address-cohort-retention',
  'average-block-time',
  'block-time-distribution',
  'capacity-turnover-ratio',
  'cell-age-vs-occupied-capacity',
  'cell-count',
  'cell-size-distribution',
  'circulation-ratio',
  'common-knowledge-composition',
  'daily-deposit',
  'difficulty',
  'epoch-time-distribution',
  'epoch-time-length',
  'hash-rate',
  'hodl-wave',
  'inflation-rate',
  'knowledge-size',
  'miner-address-distribution',
  'most-utilized-assets',
  'most-utilized-scripts',
  'nominal-apc',
  'secondary-issuance',
  'total-deposit',
  'total-supply',
  'transaction-count',
  'uncle-rate',
] as const;

export type ChartPageSlug = (typeof CHART_PAGE_SLUGS)[number];

export type ParsedMarkdownPage =
  | { kind: 'home'; pathname: '/' }
  | { kind: 'address_detail'; pathname: string; addr: string }
  | { kind: 'assets_list'; pathname: '/assets' }
  | { kind: 'blocks_list'; pathname: '/blocks' }
  | { kind: 'block_detail'; pathname: string; id: string }
  | { kind: 'cell_detail'; pathname: string; outpoint: string }
  | { kind: 'charts_overview'; pathname: '/charts' }
  | { kind: 'chart_detail'; pathname: string; slug: string }
  | { kind: 'clusters_detail'; pathname: string; clusterId: string }
  | { kind: 'dao_overview'; pathname: '/dao' }
  | { kind: 'dao_charts'; pathname: '/dao/charts' }
  | { kind: 'forks_list'; pathname: '/forks' }
  | { kind: 'fork_detail'; pathname: string; id: string }
  | { kind: 'hardforks'; pathname: '/hardforks' }
  | { kind: 'nfts_list'; pathname: '/nfts' }
  | { kind: 'nft_detail'; pathname: string; sporeId: string }
  | { kind: 'mnft_item_detail'; pathname: string; nftId: string }
  | { kind: 'script_by_code_hash'; pathname: string; codeHash: string }
  | { kind: 'scripts_list'; pathname: '/scripts' }
  | { kind: 'script_detail'; pathname: string; name: string }
  | { kind: 'tokens_list'; pathname: '/tokens' }
  | { kind: 'token_detail'; pathname: string; typeHash: string }
  | { kind: 'transactions_list'; pathname: '/transactions' }
  | { kind: 'tx_detail'; pathname: string; hash: string }
  | { kind: 'unknown'; pathname: string };

export const MARKDOWN_ROUTE_PATTERNS = [
  '/',
  '/address/{addr}',
  '/assets',
  '/blocks',
  '/blocks/{id}',
  '/cell/{outpoint}',
  '/charts',
  '/charts/{slug}',
  '/clusters/{clusterId}',
  '/dao',
  '/dao/charts',
  '/forks',
  '/forks/{id}',
  '/hardforks',
  '/nfts',
  '/nfts/{sporeId}',
  '/nfts/mnft/{nftId}',
  '/script/{codeHash}',
  '/scripts',
  '/scripts/{name}',
  '/tokens',
  '/tokens/{typeHash}',
  '/transactions',
  '/tx/{hash}',
] as const;

function normalizePathname(pathname: string): string {
  if (!pathname.startsWith('/')) {
    return `/${pathname}`;
  }
  if (pathname !== '/' && pathname.endsWith('/')) {
    return pathname.replace(/\/+$/, '');
  }
  return pathname;
}

function decodeParam(raw: string): string {
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

export function parseMarkdownSourcePath(pathname: string): ParsedMarkdownPage {
  const normalized = normalizePathname(pathname);

  if (normalized === '/') return { kind: 'home', pathname: '/' };
  if (normalized === '/assets') return { kind: 'assets_list', pathname: '/assets' };
  if (normalized === '/blocks') return { kind: 'blocks_list', pathname: '/blocks' };
  if (normalized === '/charts') return { kind: 'charts_overview', pathname: '/charts' };
  if (normalized === '/dao/charts') return { kind: 'dao_charts', pathname: '/dao/charts' };
  if (normalized === '/dao') return { kind: 'dao_overview', pathname: '/dao' };
  if (normalized === '/forks') return { kind: 'forks_list', pathname: '/forks' };
  if (normalized === '/hardforks') return { kind: 'hardforks', pathname: '/hardforks' };
  if (normalized === '/nfts') return { kind: 'nfts_list', pathname: '/nfts' };
  if (normalized === '/scripts') return { kind: 'scripts_list', pathname: '/scripts' };
  if (normalized === '/tokens') return { kind: 'tokens_list', pathname: '/tokens' };
  if (normalized === '/transactions') {
    return { kind: 'transactions_list', pathname: '/transactions' };
  }

  const addressMatch = normalized.match(/^\/address\/([^/]+)$/);
  if (addressMatch) {
    return {
      kind: 'address_detail',
      pathname: normalized,
      addr: decodeParam(addressMatch[1]),
    };
  }

  const blockMatch = normalized.match(/^\/blocks\/([^/]+)$/);
  if (blockMatch) {
    return {
      kind: 'block_detail',
      pathname: normalized,
      id: decodeParam(blockMatch[1]),
    };
  }

  const cellMatch = normalized.match(/^\/cell\/([^/]+)$/);
  if (cellMatch) {
    return {
      kind: 'cell_detail',
      pathname: normalized,
      outpoint: decodeParam(cellMatch[1]),
    };
  }

  const chartMatch = normalized.match(/^\/charts\/([^/]+)$/);
  if (chartMatch) {
    return {
      kind: 'chart_detail',
      pathname: normalized,
      slug: decodeParam(chartMatch[1]),
    };
  }

  const clusterMatch = normalized.match(/^\/clusters\/([^/]+)$/);
  if (clusterMatch) {
    return {
      kind: 'clusters_detail',
      pathname: normalized,
      clusterId: decodeParam(clusterMatch[1]),
    };
  }

  const forkMatch = normalized.match(/^\/forks\/([^/]+)$/);
  if (forkMatch) {
    return {
      kind: 'fork_detail',
      pathname: normalized,
      id: decodeParam(forkMatch[1]),
    };
  }

  const mnftItemMatch = normalized.match(/^\/nfts\/mnft\/([^/]+)$/);
  if (mnftItemMatch) {
    return {
      kind: 'mnft_item_detail',
      pathname: normalized,
      nftId: decodeParam(mnftItemMatch[1]),
    };
  }

  const nftMatch = normalized.match(/^\/nfts\/([^/]+)$/);
  if (nftMatch) {
    return {
      kind: 'nft_detail',
      pathname: normalized,
      sporeId: decodeParam(nftMatch[1]),
    };
  }

  const scriptHashMatch = normalized.match(/^\/script\/([^/]+)$/);
  if (scriptHashMatch) {
    return {
      kind: 'script_by_code_hash',
      pathname: normalized,
      codeHash: decodeParam(scriptHashMatch[1]),
    };
  }

  const scriptMatch = normalized.match(/^\/scripts\/([^/]+)$/);
  if (scriptMatch) {
    return {
      kind: 'script_detail',
      pathname: normalized,
      name: decodeParam(scriptMatch[1]),
    };
  }

  const tokenMatch = normalized.match(/^\/tokens\/([^/]+)$/);
  if (tokenMatch) {
    return {
      kind: 'token_detail',
      pathname: normalized,
      typeHash: decodeParam(tokenMatch[1]),
    };
  }

  const txMatch = normalized.match(/^\/tx\/([^/]+)$/);
  if (txMatch) {
    return {
      kind: 'tx_detail',
      pathname: normalized,
      hash: decodeParam(txMatch[1]),
    };
  }

  return {
    kind: 'unknown',
    pathname: normalized,
  };
}
