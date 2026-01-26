'use client';

import Link from 'next/link';
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
  const displayAddress =
    truncate && address.length > startChars + endChars
      ? `${address.slice(0, startChars)}...${address.slice(-endChars)}`
      : address;

  return (
    <Link
      href={`/address/${address}`}
      className={cn('text-terminal-green font-mono text-sm hover:underline', className)}
      title={address}
    >
      {displayAddress}
    </Link>
  );
}
