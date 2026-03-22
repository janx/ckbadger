'use client';

import { HelpPopover } from '@/components/ui/help-popover';
import { cn } from '@/lib/utils';

interface HMulHelpPopoverProps {
  align?: 'start' | 'end';
  className?: string;
}

const HMUL_EXPLANATION =
  'HMul = Owned Capacity / Common Knowledge. It measures how strongly a script or asset amplifies CKB hodl: for each unit of common knowledge it anchors, how much CKB is held in total. Higher HMul means stronger CKB hodl amplification.';

function parseBigInt(value: string | null | undefined): bigint | null {
  if (!value) {
    return null;
  }

  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

export function formatHMulValue(value: number | null | undefined): string | null {
  if (value == null || !Number.isFinite(value)) {
    return null;
  }

  return `×${value.toFixed(2)}`;
}

export function computeHMulValue(
  totalCapacity: string | null | undefined,
  commonKnowledgeSize: string | null | undefined
): number | null {
  const zero = BigInt(0);
  const scale = BigInt(10000);
  const total = parseBigInt(totalCapacity);
  const used = parseBigInt(commonKnowledgeSize);
  if (total == null || used == null || total <= zero || used <= zero) {
    return null;
  }

  return Number((total * scale) / used) / 10000;
}

export function HMulHelpPopover({ align = 'start', className }: HMulHelpPopoverProps) {
  return (
    <HelpPopover
      label="Explain HMul"
      title="HMul"
      align={align}
      className={className}
      contentClassName="w-[24rem] max-w-[min(24rem,calc(100vw-1rem))]"
    >
      <p>{HMUL_EXPLANATION}</p>
    </HelpPopover>
  );
}

interface HMulLabelWithHelpProps {
  align?: 'start' | 'end';
  className?: string;
  labelClassName?: string;
}

export function HMulLabelWithHelp({
  align = 'start',
  className,
  labelClassName,
}: HMulLabelWithHelpProps) {
  return (
    <span className={cn('inline-flex items-center gap-1.5', className)}>
      <span className={cn(labelClassName)}>HMul</span>
      <HMulHelpPopover align={align} />
    </span>
  );
}
