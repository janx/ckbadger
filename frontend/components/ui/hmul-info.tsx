'use client';

import { HelpPopover } from '@/components/ui/help-popover';
import { cn } from '@/lib/utils';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatScaledInteger } from '@/lib/exact-integer';

interface HMulHelpPopoverProps {
  align?: 'start' | 'end';
  className?: string;
}

const HMUL_EXPLANATION =
  'HMul = Owned Capacity / Common Knowledge. It measures how strongly a script or asset amplifies CKB hodl: for each unit of common knowledge it anchors, how much CKB is held in total. Higher HMul means stronger CKB hodl amplification.';

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
