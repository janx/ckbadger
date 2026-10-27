'use client';
import { CollectionDetail } from '@/components/asset/collection-detail';
export interface ClusterDetailPageProps {
  clusterId: string;
}
export default function ClusterDetailPage({ clusterId }: ClusterDetailPageProps) {
  return <CollectionDetail id={clusterId} kind="spore" />;
}
