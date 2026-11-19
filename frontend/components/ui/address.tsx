'use client';

import Link, { type LinkLook } from '@/components/ui/link';
import { NoBreak } from '@/components/ui/no-break';
import type { IdShortening } from '@/components/ui/hex-code';
import { shortenAddress } from '@/lib/shorten';
import { cn } from '@/lib/utils';

type AddressProps = IdShortening & {
  address: string;
  /**
   * Where it stands, which its caller says (components/ui/link.tsx): a row's key (`key`) or a name
   * inside a line (`inline`). Required (lane cd-fixes-controls, review-p-tests SHOULD-4): the look
   * it had before the looks had one owner, drawn for a caller that stated none, went with the last
   * of them.
   */
  look: Extract<LinkLook, 'key' | 'inline'>;
  className?: string;
};

export function Address({ address, look, className, ...shortening }: AddressProps) {
  const truncate = shortening.truncate !== false;
  const displayAddress =
    shortening.truncate === false ? address : shortenAddress(address, shortening.size);
  const shortened = displayAddress !== address;

  return (
    <Link
      href={`/address/${address}`}
      look={look}
      className={cn(
        'text-body font-mono transition-colors',
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
