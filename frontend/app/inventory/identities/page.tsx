import { Suspense } from 'react';
import { InventoryPageClient, InventoryPageFallback } from '../inventory-page-client';

export default function IdentitiesInventoryPage() {
  return (
    <Suspense fallback={<InventoryPageFallback />}>
      <InventoryPageClient
        assetType="identity"
        title="Identities"
        subtitle="Browse identity registrations on the CKB network"
        panelTitle="Identity List"
      />
    </Suspense>
  );
}
