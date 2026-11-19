'use client';

import { HexCode, type IdShortening } from '@/components/ui/hex-code';
import type { IdKind } from '@/lib/shorten';
import { cn } from '@/lib/utils';

type HashProps = IdShortening & {
  hash: string;
  /** An address written where a lock hash could stand (a party): `address`. */
  kind?: IdKind;
  className?: string;
  /** An id is a value: the reading ink, or bright; never quiet (R3, `HexCode`). */
  variant?: 'default' | 'bright';
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
