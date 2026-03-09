'use client';

import Link from '@/components/ui/link';
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
      className={cn(
        'text-interactive font-mono text-sm hover:underline',
        !truncate && 'inline-block max-w-full break-all',
        className
      )}
      title={address}
    >
      {displayAddress}
    </Link>
  );
}
