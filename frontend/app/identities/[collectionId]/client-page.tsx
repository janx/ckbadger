'use client';
import { CollectionDetail } from '@/components/asset/collection-detail';
export interface IdentityCollectionPageProps {
  collectionId: string;
}
export default function IdentityCollectionPage({ collectionId }: IdentityCollectionPageProps) {
  return <CollectionDetail id={collectionId} kind="identity" />;
}
