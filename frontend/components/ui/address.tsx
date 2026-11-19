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
   * inside a line (`inline`). Optional only while the link-look gate's TOLERATED list holds call
   * sites that state none; such an address draws the look it had before the looks had one owner
   * (the bright ink over a faint indigo underline), and that look goes with the last of them.
   */
  look?: Extract<LinkLook, 'key' | 'inline'>;
  className?: string;
};

/** The look of an address whose caller states none (see `look`). */
const LOOK_BEFORE = 'text-text-bright decoration-aqua/35 underline underline-offset-[3px]';

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
        look === undefined && LOOK_BEFORE,
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
