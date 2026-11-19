'use client';

import { HexCode, type IdShortening } from '@/components/ui/hex-code';
import type { IdKind } from '@/lib/shorten';
import { cn } from '@/lib/utils';

type HashProps = IdShortening & {
  hash: string;
  /** An address written where a lock hash could stand (a party): `address`. */
  kind?: IdKind;
  className?: string;
  /**
   * An id is a value: the reading ink, or bright; never quiet (R3, `HexCode`). `inherit` writes no
   * ink of its own: a hash that is a row's key stands in a link of the key look, whose ink it takes
   * (the link-look gate: a link's ink is its look's, never a child's).
   */
  variant?: 'default' | 'bright' | 'inherit';
};

export function Hash({ hash, kind, className, variant = 'default', ...shortening }: HashProps) {
  return (
    <HexCode
      value={hash}
      kind={kind}
      {...shortening}
      className={cn('text-body', className)}
      variant={variant}
    />
  );
}
