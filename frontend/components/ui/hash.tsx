'use client';

import { HexCode } from '@/components/ui/hex-code';
import { cn } from '@/lib/utils';

interface HashProps {
  hash: string;
  truncate?: boolean;
  startChars?: number;
  endChars?: number;
  className?: string;
  copyable?: boolean;
  variant?: 'default' | 'bright' | 'dim';
}

export function Hash({
  hash,
  truncate = true,
  startChars = 10,
  endChars = 8,
  className,
  copyable = true,
  variant = 'default',
}: HashProps) {
  return (
    <HexCode
      value={hash}
      truncate={truncate}
      startChars={startChars}
      endChars={endChars}
      className={cn('text-sm', className)}
      copyable={copyable}
      variant={variant}
    />
  );
}
