'use client';

import Link from '@/components/ui/link';
import { HexDisplay } from '@/components/ui/hex-display';
import type { CollectionHolder } from '@/lib/api';

/**
 * One holder in a collection's holder list: its address when the lock encodes
 * to one, else its lock hash — both link to the address page. A holder the API
 * can name but not address has no lock hash; linking one anyway would invent
 * an address, so it is shown unlinked.
 */
export function HolderLabel({ holder }: { holder: CollectionHolder }) {
  if (holder.address) {
    return (
      <Link
        href={`/address/${holder.address}`}
        className="text-text font-mono text-xs hover:underline"
      >
        {holder.address}
      </Link>
    );
  }
  if (holder.lockScriptHash) {
    return (
      <Link
        href={`/address/${holder.lockScriptHash}`}
        className="text-text font-mono text-xs hover:underline"
      >
        <HexDisplay value={holder.lockScriptHash} size="sm" startChars={12} endChars={10} />
      </Link>
    );
  }
  return <span className="text-text-dim font-mono text-xs">Unknown holder</span>;
}
