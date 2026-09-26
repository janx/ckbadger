'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import type { Cell, Token } from '@/lib/api';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { formatNumber, truncateHash } from '@/lib/utils';

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

type InventoryItemType =
  | 'spore'
  | 'cluster'
  | 'mnft_token'
  | 'mnft_class'
  | 'mnft_issuer'
  | 'udt'
  | 'dotbit'
  | 'did_ckb'
  | 'dotcell'
  | 'dotcell_ring';

interface InventoryContext {
  itemType: InventoryItemType;
  itemId: string;
}

// Registry protocol slugs the API serves as `cell.protocolScript` — the only
// way this module recognises a protocol that has no deterministic decode.
// No code hashes here: they differ per network and the registry owns them.
const DID_CKB_SLUG = 'did-ckb';
const DOTCELL_ACCOUNT_SLUG = 'dotcell-account';

const DETERMINISTIC_KIND_MAP: Record<string, InventoryItemType> = {
  spore_cell: 'spore',
  spore_cluster_cell: 'cluster',
  mnft_token_cell: 'mnft_token',
  mnft_class_cell: 'mnft_class',
  mnft_issuer_cell: 'mnft_issuer',
  udt_amount: 'udt',
  dotbit_account: 'dotbit',
  dotcell_name: 'dotcell',
  dotcell_ring_root: 'dotcell_ring',
};

const DAO_KINDS = new Set(['dao_deposit_cell', 'dao_withdraw_request_cell']);

/**
 * The `.cell` name from the decoded `label` segment, whose value reads
 * `support.cell · id 0x…` — the part before the first ` · `. `null` when the
 * cell carries no decoded label (no direct cell-data reader, or the ring root).
 */
function dotcellNameFromSegments(cell: Cell): string | null {
  const segment = cell.dataAnalysis?.deterministic?.segments?.find((s) => s.label === 'label');
  if (!segment?.humanValue) return null;
  const name = segment.humanValue.split(' · ')[0];
  return name.endsWith('.cell') ? name : null;
}

function detectItemId(cell: Cell, itemType: InventoryItemType): string {
  switch (itemType) {
    case 'udt':
      return cell.typeScriptHash ?? '';
    case 'dotcell': {
      // The identity route accepts a name; the type args are the namespace,
      // shared by every name, not this name's id.
      const name = dotcellNameFromSegments(cell);
      return name ? name.slice(0, -'.cell'.length) : '';
    }
    case 'dotcell_ring':
      return '';
    default:
      return cell.type?.args ?? '';
  }
}

function detectInventoryContext(cell: Cell): InventoryContext | null {
  if (!cell.type) return null;

  const kind = cell.dataAnalysis?.deterministic?.kind;

  if (kind) {
    if (DAO_KINDS.has(kind)) return null;

    const itemType = DETERMINISTIC_KIND_MAP[kind];
    if (!itemType) return null;

    return { itemType, itemId: detectItemId(cell, itemType) };
  }

  switch (cell.protocolScript?.type) {
    case DID_CKB_SLUG:
      return { itemType: 'did_ckb', itemId: cell.type.args };
    case DOTCELL_ACCOUNT_SLUG:
      // No decoded data (no direct cell-data reader): the protocol is known,
      // the name is not.
      return { itemType: 'dotcell', itemId: '' };
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Inline label data
// ---------------------------------------------------------------------------

export interface InventoryLabel {
  /** Generic type label, e.g. "Token (UDT)", "Spore Object" */
  typeLabel: string;
  /** Concrete item name when available, e.g. "@PalofSeal (Otter)", "alice.bit" */
  displayName: string | null;
  /** Inline summary of this cell's payload, e.g. "123,456.789 TT" */
  summary: string | null;
  /** Link to the item detail page */
  href: string | null;
}

function getTypeLabel(itemType: InventoryItemType): string {
  switch (itemType) {
    case 'spore':
      return 'Spore Object';
    case 'cluster':
      return 'Spore Cluster';
    case 'mnft_token':
      return 'M-NFT Token';
    case 'mnft_class':
      return 'M-NFT Class';
    case 'mnft_issuer':
      return 'M-NFT Issuer';
    case 'udt':
      return 'Token (UDT)';
    case 'dotbit':
      return '.bit Account';
    case 'did_ckb':
      return 'DID:CKB Identity';
    case 'dotcell':
      return '.cell Name';
    case 'dotcell_ring':
      return '.cell Ring Root';
  }
}

function getHref(ctx: InventoryContext): string | null {
  switch (ctx.itemType) {
    case 'spore':
      return `/objects/${ctx.itemId}`;
    case 'cluster':
      return `/clusters/${ctx.itemId}`;
    case 'mnft_token':
      return `/objects/mnft/${ctx.itemId}`;
    case 'mnft_class':
      return `/classes/${ctx.itemId}`;
    case 'udt':
      return `/tokens/${ctx.itemId}`;
    case 'dotbit':
      return `/identities/dotbit/${ctx.itemId}`;
    case 'did_ckb':
      return `/identities/did/${ctx.itemId}`;
    case 'dotcell':
      return ctx.itemId ? getIdentityItemDetailHref('dotcell', ctx.itemId) : '/identities/dotcell';
    case 'dotcell_ring':
      return '/identities/dotcell';
    case 'mnft_issuer':
      return null;
  }
}

// ---------------------------------------------------------------------------
// Build summary text per type (cell-level info only)
// ---------------------------------------------------------------------------

function buildUdtDisplayName(token: Token): string | null {
  return token.symbol || token.name || null;
}

function buildUdtSummary(token: Token, cell: Cell): string | null {
  if (!cell.udtAmount) return null;
  const amount = formatTokenBalanceWithRawMarker(cell.udtAmount, token.decimals);
  return token.symbol ? `${amount} ${token.symbol}` : amount;
}

// ---------------------------------------------------------------------------
// Hook: useInventoryLabel
// ---------------------------------------------------------------------------

export function useInventoryLabel(cell: Cell | undefined | null): InventoryLabel | null {
  const ctx = cell ? detectInventoryContext(cell) : null;

  // Only UDT needs an API call to get token name/symbol/decimals.
  // Other types can derive their summary from the cell itself.
  const { data: tokenData } = useQuery({
    queryKey: ['inventory-label-token', ctx?.itemId],
    queryFn: () => api.getToken(ctx!.itemId),
    enabled: ctx?.itemType === 'udt',
    staleTime: Infinity,
  });

  if (!ctx || !cell) return null;

  const typeLabel = getTypeLabel(ctx.itemType);
  const href = getHref(ctx);

  let displayName: string | null = null;
  let summary: string | null = null;

  switch (ctx.itemType) {
    case 'udt': {
      if (tokenData) {
        displayName = buildUdtDisplayName(tokenData);
        summary = buildUdtSummary(tokenData, cell);
      }
      break;
    }
    case 'spore': {
      const det = cell.dataAnalysis?.deterministic;
      if (det) {
        const parts: string[] = [];
        const contentTypeSeg = det.segments?.find((s) => s.label === 'content_type');
        if (contentTypeSeg?.humanValue) parts.push(contentTypeSeg.humanValue);
        const sizeSeg = det.segments?.find((s) => s.label === 'content');
        if (sizeSeg) parts.push(`${formatNumber(sizeSeg.end - sizeSeg.start)} bytes`);
        summary = parts.length > 0 ? parts.join(' · ') : null;
      }
      break;
    }
    case 'cluster': {
      const det = cell.dataAnalysis?.deterministic;
      const nameSeg = det?.segments?.find((s) => s.label === 'name');
      if (nameSeg?.humanValue) displayName = nameSeg.humanValue;
      break;
    }
    case 'dotbit': {
      const det = cell.dataAnalysis?.deterministic;
      const nameSeg = det?.segments?.find((s) => s.label === 'account');
      if (nameSeg?.humanValue) displayName = nameSeg.humanValue;
      break;
    }
    case 'mnft_token': {
      const det = cell.dataAnalysis?.deterministic;
      const indexSeg = det?.segments?.find((s) => s.label === 'token_index');
      if (indexSeg?.humanValue) displayName = `Token #${indexSeg.humanValue}`;
      break;
    }
    case 'dotcell': {
      displayName = dotcellNameFromSegments(cell);
      const segments = cell.dataAnalysis?.deterministic?.segments;
      const parts: string[] = [];
      const ownerSeg = segments?.find((s) => s.label === 'owner_hash20');
      if (ownerSeg?.humanValue) parts.push(`owner ${truncateHash(ownerSeg.humanValue, 6, 4)}`);
      const expirySeg = segments?.find((s) => s.label === 'expired_at');
      // `2027-09-21T06:21:18+00:00 (unix 1821507678)` → the UTC date.
      if (expirySeg?.humanValue) parts.push(`expires ${expirySeg.humanValue.split('T')[0]}`);
      summary = parts.length > 0 ? parts.join(' · ') : null;
      break;
    }
    default:
      break;
  }

  return { typeLabel, displayName, summary, href };
}
