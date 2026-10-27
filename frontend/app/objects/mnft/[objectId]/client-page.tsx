'use client';
import { MnftDetail } from '@/components/asset/mnft-detail';
import { normalizeAssetId } from '@/lib/asset-utils';

export interface MnftItemDetailPageProps {
  objectId: string;
}
export default function MnftItemDetailPage({ objectId }: MnftItemDetailPageProps) {
  const id = normalizeAssetId(objectId);
  return <MnftDetail key={id} objectId={id} />;
}
