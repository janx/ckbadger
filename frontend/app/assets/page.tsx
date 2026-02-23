import { Suspense } from 'react';
import { AssetsPageClient, AssetsPageFallback } from './assets-page-client';

export default function AssetsPage() {
  return (
    <Suspense fallback={<AssetsPageFallback />}>
      <AssetsPageClient />
    </Suspense>
  );
}
