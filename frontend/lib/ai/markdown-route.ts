import { CHART_ROUTES, PAGE_ROUTES, matchPageRoute } from '@/lib/ai/page-registry';

export const CHART_PAGE_SLUGS = Object.keys(CHART_ROUTES) as (keyof typeof CHART_ROUTES)[];

export type ChartPageSlug = (typeof CHART_PAGE_SLUGS)[number];

export type ParsedMarkdownPage =
  | { kind: 'home'; pathname: '/' }
  | { kind: 'activities_list'; pathname: '/activities' }
  | { kind: 'address_detail'; pathname: string; addr: string }
  | { kind: 'inventory_tokens'; pathname: '/inventory/tokens' }
  | { kind: 'inventory_objects'; pathname: '/inventory/objects' }
  | { kind: 'inventory_identities'; pathname: '/inventory/identities' }
  | { kind: 'blocks_list'; pathname: '/blocks' }
  | { kind: 'block_detail'; pathname: string; id: string }
  | { kind: 'cell_detail'; pathname: string; outpoint: string }
  | { kind: 'charts_overview'; pathname: '/charts' }
  | { kind: 'chart_detail'; pathname: string; slug: string }
  | { kind: 'classes_detail'; pathname: string; classId: string }
  | { kind: 'clusters_detail'; pathname: string; clusterId: string }
  | { kind: 'dao_overview'; pathname: '/dao' }
  | { kind: 'dao_charts'; pathname: '/dao/charts' }
  | { kind: 'forks_list'; pathname: '/forks' }
  | { kind: 'fork_detail'; pathname: string; id: string }
  | { kind: 'hardforks'; pathname: '/hardforks' }
  | { kind: 'network_overview'; pathname: '/network' }
  | { kind: 'identity_collection'; pathname: string; collectionId: string }
  | { kind: 'objects_list'; pathname: '/objects' }
  | { kind: 'object_detail'; pathname: string; sporeId: string }
  | { kind: 'dotbit_item_detail'; pathname: string; identityId: string }
  | { kind: 'did_ckb_item_detail'; pathname: string; identityId: string }
  | { kind: 'bit_cell_item_detail'; pathname: string; identityId: string }
  | { kind: 'mnft_item_detail'; pathname: string; objectId: string }
  | { kind: 'script_by_code_hash'; pathname: string; codeHash: string }
  | { kind: 'scripts_list'; pathname: '/scripts' }
  | { kind: 'script_detail'; pathname: string; name: string }
  | { kind: 'tokens_list'; pathname: '/tokens' }
  | { kind: 'token_detail'; pathname: string; typeHash: string }
  | { kind: 'transactions_list'; pathname: '/transactions' }
  | { kind: 'tx_detail'; pathname: string; hash: string }
  | { kind: 'fiber_channels_list'; pathname: '/fiber/channels' }
  | { kind: 'fiber_channel_detail'; pathname: string; channelId: string }
  | { kind: 'unknown'; pathname: string };

export const MARKDOWN_ROUTE_PATTERNS = PAGE_ROUTES.map((route) => route.pattern);

export function parseMarkdownSourcePath(pathname: string): ParsedMarkdownPage {
  return matchPageRoute(pathname);
}
