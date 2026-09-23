'use client';

import { DotCellItemDetail } from '@/components/identity/dotcell-item-detail';

export interface DotCellItemDetailPageProps {
  identityId: string;
}

export default function DotCellItemDetailPage({ identityId }: DotCellItemDetailPageProps) {
  return <DotCellItemDetail identityId={identityId} />;
}
