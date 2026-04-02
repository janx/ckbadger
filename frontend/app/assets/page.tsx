'use client';

import { useEffect } from 'react';
import { useSearchParams, useRouter } from '@/src/navigation';

export default function AssetsRedirectPage() {
  const searchParams = useSearchParams();
  const router = useRouter();

  useEffect(() => {
    const type = searchParams.get('type');
    if (type === 'object' || type === 'dob') {
      router.replace('/inventory/objects');
    } else if (type === 'identity') {
      router.replace('/inventory/identities');
    } else {
      router.replace('/inventory/tokens');
    }
  }, [searchParams, router]);

  return null;
}
