'use client';

import { HelpPopover } from '@/components/ui/help-popover';
import { cn } from '@/lib/utils';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatScaledInteger } from '@/lib/exact-integer';

interface HMulHelpPopoverProps {
  align?: 'start' | 'end';
  className?: string;
}

// F2 objects (IV-07): the boards' words — CKB held ÷ occupied.
const HMUL_EXPLANATION =
  'H-multiplier = CKB held in its cells ÷ occupied. Occupied is the CKB that pays for the bytes of its cells, the state it keeps on CKB; for each of those CKB, this many are held in the same cells. Higher means more CKB parked alongside its state.';

export function formatHMulValue(value: string | null | undefined): string | null {
  return value == null ? null : `×${value}`;
}

export function computeHMulValue(
  totalCapacity: string | null | undefined,
  commonKnowledgeSize: string | null | undefined
): string | null {
  const zero = BigInt(0);
  if (totalCapacity == null || commonKnowledgeSize == null) return null;
  const total = parseShannons(totalCapacity, 'totalCapacity');
  const used = parseShannons(commonKnowledgeSize, 'commonKnowledgeSize');
  if (total < zero || used < zero || used > total)
    throw new Error(`invalid HMul capacity split ${total}/${used}`);
  if (used === zero) return null;
  return formatScaledInteger(total, used, 2);
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
