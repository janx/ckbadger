'use client';

import Link from '@/components/ui/link';
import { NoBreak } from '@/components/ui/no-break';
import type { IdShortening } from '@/components/ui/hex-code';
import { shortenAddress } from '@/lib/shorten';
import { cn } from '@/lib/utils';

type AddressProps = IdShortening & {
  address: string;
  className?: string;
};

export function Address({ address, className, ...shortening }: AddressProps) {
  const truncate = shortening.truncate !== false;
  const displayAddress =
    shortening.truncate === false ? address : shortenAddress(address, shortening.size);
  const shortened = displayAddress !== address;

  return (
    <Link
      href={`/address/${address}`}
      className={cn(
        'text-text-bright decoration-aqua/35 text-body font-mono underline underline-offset-[3px] transition-colors',
        !truncate && 'inline-block max-w-full break-all',
        className
      )}
      title={address}
    >
      {/* U-16: a shortened address is one unit; it never breaks at its ellipsis. */}
      {shortened ? <NoBreak>{displayAddress}</NoBreak> : displayAddress}
    </Link>
  );
}
