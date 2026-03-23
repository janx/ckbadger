import { Suspense } from 'react';
import { InventoryPageClient, InventoryPageFallback } from '../inventory-page-client';

export default function TokensInventoryPage() {
  return (
    <Suspense fallback={<InventoryPageFallback />}>
      <InventoryPageClient
        assetType="token"
        title="Tokens"
        subtitle="Browse fungible tokens on the CKB network"
        panelTitle="Token List"
      />
    </Suspense>
  );
}
