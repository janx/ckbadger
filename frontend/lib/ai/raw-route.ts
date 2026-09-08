import { PAGE_ROUTES, matchPageRoute } from '@/lib/ai/page-registry';

export type ParsedRawPage =
  | { kind: 'block_detail'; pathname: string; id: string }
  | { kind: 'cell_detail'; pathname: string; outpoint: string }
  | { kind: 'dotbit_item_detail'; pathname: string; identityId: string }
  | { kind: 'did_ckb_item_detail'; pathname: string; identityId: string }
  | { kind: 'bit_cell_item_detail'; pathname: string; identityId: string }
  | { kind: 'mnft_item_detail'; pathname: string; objectId: string }
  | { kind: 'tx_detail'; pathname: string; hash: string }
  | { kind: 'unknown'; pathname: string };

export const RAW_ROUTE_PATTERNS = PAGE_ROUTES.filter((route) => route.rawProfiles.length > 0).map(
  (route) => route.pattern
);

export function parseRawSourcePath(pathname: string): ParsedRawPage {
  return matchPageRoute(pathname, true) as ParsedRawPage;
}
