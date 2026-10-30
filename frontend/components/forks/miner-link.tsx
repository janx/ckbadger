'use client';

import Link from '@/components/ui/link';

/** A miner by its address tail (pool names are not available, R2b-b-12). */
export function MinerLink({ address }: { address: string }) {
  return (
    <Link href={`/address/${address}`} className="text-text-bright hover:text-jade" title={address}>
      …{address.slice(-6)}
    </Link>
  );
}
