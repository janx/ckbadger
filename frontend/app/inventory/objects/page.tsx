import { Suspense } from 'react';
import { InventoryPageClient, InventoryPageFallback } from '../inventory-page-client';

export default function ObjectsInventoryPage() {
  return (
    <Suspense fallback={<InventoryPageFallback />}>
      <InventoryPageClient
        assetType="object"
        title="Objects"
        subtitle="Browse digital objects and collections on the CKB network"
        panelTitle="Object List"
      />
    </Suspense>
  );
}
