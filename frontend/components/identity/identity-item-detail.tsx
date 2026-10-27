'use client';
import { useQuery } from '@tanstack/react-query';
import type { CursorPaginatedResponse, MnftItemActivity, CollectionItem } from '@/lib/api';
import { isNotFoundApiError } from '@/lib/api';
import { normalizeAssetId } from '@/lib/asset-utils';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
export interface IdentityItemDetailConfig {
  standard: 'dotbit' | 'did_ckb' | 'bit_cell';
  fetchDetail: (identityId: string) => Promise<CollectionItem>;
  fetchActivities: (
    identityId: string,
    params: { limit: number; cursor?: string }
  ) => Promise<CursorPaginatedResponse<MnftItemActivity>>;
  labels: {
    standardDisplay: string;
    nameLabel: string;
    idLabel: string;
    backLabel: string;
    backHref: string;
    defaultTitle: string;
    notFoundMsg: string;
    recycledMsg: string;
    showExpiry: boolean;
    showVisualization?: boolean;
  };
}

interface Props {
  config: IdentityItemDetailConfig;
  identityId: string;
}
export function IdentityItemDetail({ config, identityId }: Props) {
  const id = normalizeAssetId(identityId);
  return <ScopedIdentity key={id} config={config} identityId={id} />;
}
function ScopedIdentity({ config, identityId }: Props) {
  const detail = useQuery({
    queryKey: [`${config.standard}-item-detail`, identityId],
    queryFn: () => config.fetchDetail(identityId),
    retry: false,
  });
  const item = detail.data;
  if (!item)
    return (
      <AssetDetailShell
        title={
          isNotFoundApiError(detail.error) ? config.labels.notFoundMsg : config.labels.defaultTitle
        }
        kind="identities"
        breadcrumb={[{ label: config.labels.backLabel, href: config.labels.backHref }]}
      >
        <AssetReadState label="identity" error={detail.error} loading={detail.isLoading} />
      </AssetDetailShell>
    );
  return (
    <CanonicalIdentityDetail
      kind={
        config.standard === 'did_ckb'
          ? 'did'
          : config.standard === 'bit_cell'
            ? 'bit-cell'
            : 'dotbit'
      }
      id={item.nftId}
      title={item.name ?? config.labels.defaultTitle}
      isLive={item.isLive}
      createdAtBlock={item.createdAtBlock}
      error={detail.error}
    />
  );
}
