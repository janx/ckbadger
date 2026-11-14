'use client';

import { HexCode, type IdShortening } from '@/components/ui/hex-code';
import type { IdKind } from '@/lib/shorten';
import { cn } from '@/lib/utils';

type HashProps = IdShortening & {
  hash: string;
  /** An address written where a lock hash could stand (a party): `address`. */
  kind?: IdKind;
  className?: string;
  copyable?: boolean;
  variant?: 'default' | 'bright' | 'dim';
};

export function Hash({
  hash,
  kind,
  className,
  copyable = true,
  variant = 'default',
  ...shortening
}: HashProps) {
  return (
    <HexCode
      value={hash}
      kind={kind}
      {...shortening}
      className={cn('text-body', className)}
      copyable={copyable}
      variant={variant}
    />
  );
}
