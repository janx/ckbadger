'use client';
import { CollectionDetail } from '@/components/asset/collection-detail';
export interface MnftClassDetailPageProps {
  classId: string;
}
export default function MnftClassDetailPage({ classId }: MnftClassDetailPageProps) {
  return <CollectionDetail id={classId} kind="object" />;
}
