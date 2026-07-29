'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import type { Cell, Token } from '@/lib/api';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { formatNumber } from '@/lib/utils';

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
  | 'did_ckb';

interface InventoryContext {
  itemType: InventoryItemType;
  itemId: string;
}

const DID_CKB_CODE_HASH = '0x079bb8c1dfb249f60d932f4b1a60fa5cb2a36af3653ac09464f262e2f3f682a9';

const DETERMINISTIC_KIND_MAP: Record<string, InventoryItemType> = {
  spore_cell: 'spore',
  spore_cluster_cell: 'cluster',
  mnft_token_cell: 'mnft_token',
  mnft_class_cell: 'mnft_class',
  mnft_issuer_cell: 'mnft_issuer',
  udt_amount: 'udt',
  dotbit_account: 'dotbit',
};

const DAO_KINDS = new Set(['dao_deposit_cell', 'dao_withdraw_request_cell']);

function detectInventoryContext(cell: Cell): InventoryContext | null {
  if (!cell.type) return null;

  const kind = cell.dataAnalysis?.deterministic?.kind;

  if (kind) {
    if (DAO_KINDS.has(kind)) return null;

    const itemType = DETERMINISTIC_KIND_MAP[kind];
    if (!itemType) return null;

    const itemId = itemType === 'udt' ? (cell.typeScriptHash ?? '') : cell.type.args;

    return { itemType, itemId };
  }

  if (cell.type.codeHash === DID_CKB_CODE_HASH) {
    return { itemType: 'did_ckb', itemId: cell.type.args };
  }

  return null;
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
    default:
      break;
  }

  return { typeLabel, displayName, summary, href };
}
