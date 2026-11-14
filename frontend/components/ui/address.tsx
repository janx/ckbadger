'use client';

import Link from '@/components/ui/link';
import { NoBreak } from '@/components/ui/no-break';
import { cn } from '@/lib/utils';

interface AddressProps {
  address: string;
  truncate?: boolean;
  startChars?: number;
  endChars?: number;
  className?: string;
}

export function Address({
  address,
  truncate = true,
  startChars = 12,
  endChars = 8,
  className,
}: AddressProps) {
  const shortened = truncate && address.length > startChars + endChars;
  const displayAddress = shortened
    ? `${address.slice(0, startChars)}...${address.slice(-endChars)}`
    : address;

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
