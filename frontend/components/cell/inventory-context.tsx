'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { api } from '@/lib/api';
import type {
  Cell,
  SporeNft,
  SporeCluster,
  Token,
  MnftItemDetail,
  ObjectCollection,
  CollectionItem,
} from '@/lib/api';
import { Address } from '@/components/ui/address';
import { HexDisplay } from '@/components/ui/hex-display';
import { Badge } from '@/components/ui/page-header';
import { formatTokenBalance } from '@/lib/format-asset';
import { formatNumber } from '@/lib/utils';

// ---------------------------------------------------------------------------
// Detection hook
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

function useInventoryContext(cell: Cell): InventoryContext | null {
  if (!cell.type) return null;

  const kind = cell.dataAnalysis?.deterministic?.kind;

  if (kind) {
    if (DAO_KINDS.has(kind)) return null;

    const itemType = DETERMINISTIC_KIND_MAP[kind];
    if (!itemType) return null;

    const itemId = itemType === 'udt' ? (cell.typeScriptHash ?? '') : cell.type.args;

    return { itemType, itemId };
  }

  // Fallback: DID CKB detection by code_hash
  if (cell.type.codeHash === DID_CKB_CODE_HASH) {
    return { itemType: 'did_ckb', itemId: cell.type.args };
  }

  return null;
}

// ---------------------------------------------------------------------------
// Data fetching
// ---------------------------------------------------------------------------

type InventoryData =
  | { type: 'spore'; data: SporeNft }
  | { type: 'cluster'; data: SporeCluster }
  | { type: 'udt'; data: Token }
  | { type: 'mnft_token'; data: MnftItemDetail }
  | { type: 'mnft_class'; data: ObjectCollection }
  | { type: 'mnft_issuer'; data: ObjectCollection }
  | { type: 'dotbit'; data: CollectionItem }
  | { type: 'did_ckb'; data: CollectionItem };

async function fetchInventoryData(ctx: InventoryContext): Promise<InventoryData> {
  switch (ctx.itemType) {
    case 'spore':
      return { type: 'spore', data: await api.getSporeObject(ctx.itemId) };
    case 'cluster':
      return { type: 'cluster', data: await api.getSporeCluster(ctx.itemId) };
    case 'udt':
      return { type: 'udt', data: await api.getToken(ctx.itemId) };
    case 'mnft_token':
      return {
        type: 'mnft_token',
        data: await api.getMnftItemDetail(ctx.itemId),
      };
    case 'mnft_class':
      return {
        type: 'mnft_class',
        data: await api.getObjectCollection(ctx.itemId),
      };
    case 'mnft_issuer':
      return {
        type: 'mnft_issuer',
        data: await api.getObjectCollection(ctx.itemId),
      };
    case 'dotbit':
      return {
        type: 'dotbit',
        data: await api.getDotbitItemDetail(ctx.itemId),
      };
    case 'did_ckb':
      return {
        type: 'did_ckb',
        data: await api.getDidCkbItemDetail(ctx.itemId),
      };
  }
}

// ---------------------------------------------------------------------------
// Link targets
// ---------------------------------------------------------------------------

function getViewDetailsHref(ctx: InventoryContext): string | null {
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
// Type labels
// ---------------------------------------------------------------------------

function getTypeLabel(itemType: InventoryItemType): string {
  switch (itemType) {
    case 'spore':
      return 'Spore NFT';
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

// ---------------------------------------------------------------------------
// Skeleton
// ---------------------------------------------------------------------------

function InventoryContextSkeleton() {
  return (
    <div data-testid="inventory-context-loading" className="animate-pulse">
      <div className="bg-base-elevated h-32 rounded-lg" />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Per-type card components
// ---------------------------------------------------------------------------

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-text-dim text-xs">{label}</div>
      <div className="text-text-bright">{children}</div>
    </div>
  );
}

const COMPOSITION_TIER_LABELS: Record<string, string> = {
  btc_ckb: 'BTC+CKB',
  pure_ckb: 'Pure CKB',
  decentralized_mixture: 'Decentralized Mixture',
  centralized_mixture: 'Centralized Mixture',
  unknown: 'Unknown',
};

function SporeCard({ data }: { data: SporeNft }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      <InfoRow label="Content Type">{data.contentType}</InfoRow>
      <InfoRow label="Content Size">{formatNumber(data.contentSize)} bytes</InfoRow>
      {data.clusterId && (
        <InfoRow label="Cluster">
          <Link href={`/clusters/${data.clusterId}`} className="text-aqua hover:underline">
            {data.clusterId}
          </Link>
        </InfoRow>
      )}
      <InfoRow label="Owner">
        {data.ownerAddress ? (
          <Address address={data.ownerAddress} />
        ) : (
          <HexDisplay value={data.ownerLockHash} size="sm" />
        )}
      </InfoRow>
      {data.mediaProfile?.tier && (
        <InfoRow label="Composition Tier">
          <Badge variant="neutral">
            {COMPOSITION_TIER_LABELS[data.mediaProfile.tier] ?? data.mediaProfile.tier}
          </Badge>
        </InfoRow>
      )}
    </div>
  );
}

function ClusterCard({ data }: { data: SporeCluster }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Name">{data.name}</InfoRow>}
      <InfoRow label="Spores Count">{formatNumber(data.sporesCount)}</InfoRow>
      <InfoRow label="Holders">{formatNumber(data.holdersCount)}</InfoRow>
      {data.description && (
        <InfoRow label="Description">
          <span className="line-clamp-2">{data.description}</span>
        </InfoRow>
      )}
    </div>
  );
}

function UdtCard({ data, cell }: { data: Token; cell: Cell }) {
  const formattedAmount = cell.udtAmount ? formatTokenBalance(cell.udtAmount, data.decimals) : null;

  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Token">{data.name}</InfoRow>}
      {data.symbol && <InfoRow label="Symbol">{data.symbol}</InfoRow>}
      {formattedAmount && <InfoRow label="Amount">{formattedAmount}</InfoRow>}
      <InfoRow label="Total Supply">{formatTokenBalance(data.totalSupply, data.decimals)}</InfoRow>
      <InfoRow label="Holders">{formatNumber(data.holdersCount)}</InfoRow>
    </div>
  );
}

function MnftTokenCard({ data }: { data: MnftItemDetail }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.class?.name && (
        <InfoRow label="Class">
          <Link href={`/classes/${data.class.classId}`} className="text-aqua hover:underline">
            {data.class.name}
          </Link>
        </InfoRow>
      )}
      {data.issuer?.name && <InfoRow label="Issuer">{data.issuer.name}</InfoRow>}
      <InfoRow label="Token Index">{formatNumber(data.tokenIndex)}</InfoRow>
      <InfoRow label="Characteristics">
        <HexDisplay value={data.characteristicHex} size="sm" />
      </InfoRow>
      {data.ownerLockHash && (
        <InfoRow label="Owner">
          <HexDisplay value={data.ownerLockHash} size="sm" />
        </InfoRow>
      )}
    </div>
  );
}

function MnftClassCard({ data }: { data: ObjectCollection }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Name">{data.name}</InfoRow>}
      <InfoRow label="Total Items">{formatNumber(data.totalCount)}</InfoRow>
      <InfoRow label="Holders">{formatNumber(data.holdersCount)}</InfoRow>
      {data.issuerDetail?.name && <InfoRow label="Issuer Name">{data.issuerDetail.name}</InfoRow>}
      {data.classDetail != null && (
        <InfoRow label="Supply">
          {formatNumber(data.classDetail.issued)} / {formatNumber(data.classDetail.total)}
        </InfoRow>
      )}
    </div>
  );
}

function MnftIssuerCard({ data }: { data: ObjectCollection }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.issuerDetail?.name && <InfoRow label="Issuer">{data.issuerDetail.name}</InfoRow>}
      {data.issuerDetail?.classCount != null && (
        <InfoRow label="Classes">{formatNumber(data.issuerDetail.classCount)}</InfoRow>
      )}
      {data.issuerDetail?.setCount != null && (
        <InfoRow label="Set Count">{formatNumber(data.issuerDetail.setCount)}</InfoRow>
      )}
    </div>
  );
}

function DotbitCard({ data }: { data: CollectionItem }) {
  const expiryDate =
    data.expiredAt != null ? new Date(data.expiredAt * 1000).toLocaleDateString() : null;

  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Account">{data.name}</InfoRow>}
      <InfoRow label="Standard">{data.standard}</InfoRow>
      {data.ownerLockHash && (
        <InfoRow label="Owner">
          <HexDisplay value={data.ownerLockHash} size="sm" />
        </InfoRow>
      )}
      {expiryDate && <InfoRow label="Expiry">{expiryDate}</InfoRow>}
    </div>
  );
}

function DidCkbCard({ data }: { data: CollectionItem }) {
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      {data.name && <InfoRow label="Identity">{data.name}</InfoRow>}
      <InfoRow label="Standard">{data.standard}</InfoRow>
      {data.ownerLockHash && (
        <InfoRow label="Owner">
          <HexDisplay value={data.ownerLockHash} size="sm" />
        </InfoRow>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Card renderer
// ---------------------------------------------------------------------------

function renderCard(inv: InventoryData, cell: Cell): React.ReactNode {
  switch (inv.type) {
    case 'spore':
      return <SporeCard data={inv.data} />;
    case 'cluster':
      return <ClusterCard data={inv.data} />;
    case 'udt':
      return <UdtCard data={inv.data} cell={cell} />;
    case 'mnft_token':
      return <MnftTokenCard data={inv.data} />;
    case 'mnft_class':
      return <MnftClassCard data={inv.data} />;
    case 'mnft_issuer':
      return <MnftIssuerCard data={inv.data} />;
    case 'dotbit':
      return <DotbitCard data={inv.data} />;
    case 'did_ckb':
      return <DidCkbCard data={inv.data} />;
  }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

export function InventoryContextSection({ cell }: { cell: Cell }) {
  const ctx = useInventoryContext(cell);

  const { data, isLoading, isError } = useQuery({
    queryKey: ['inventory-context', ctx?.itemType, ctx?.itemId],
    queryFn: () => fetchInventoryData(ctx!),
    enabled: !!ctx,
  });

  // No type script or unrecognized kind
  if (!ctx) return null;

  // Loading state
  if (isLoading) return <InventoryContextSkeleton />;

  // Error — hide silently
  if (isError || !data) return null;

  const href = getViewDetailsHref(ctx);

  return (
    <div data-testid="inventory-context-section">
      <TerminalPanel>
        <TerminalPanelHeader
          actions={
            href ? (
              <Link href={href} className="text-aqua text-sm hover:underline">
                View details &rarr;
              </Link>
            ) : undefined
          }
        >
          {getTypeLabel(ctx.itemType)}
        </TerminalPanelHeader>
        <TerminalPanelContent>{renderCard(data, cell)}</TerminalPanelContent>
      </TerminalPanel>
    </div>
  );
}
