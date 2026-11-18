'use client';

import { HelpPopover } from '@/components/ui/help-popover';

interface HMulHelpPopoverProps {
  align?: 'start' | 'end';
  className?: string;
}

// F2 objects (IV-07): the boards' words — CKB held ÷ occupied.
export const HMUL_EXPLANATION =
  'H-multiplier = CKB held in its cells ÷ occupied. Occupied is the CKB that pays for the bytes of its cells, the state it keeps on CKB; for each of those CKB, this many are held in the same cells. Higher means more CKB parked alongside its state.';

export function HMulHelpPopover({ align = 'start', className }: HMulHelpPopoverProps) {
  return (
    <HelpPopover
      label="Explain HMul"
      title="HMul"
      align={align}
      className={className}
      contentClassName="w-[24rem] max-w-[min(24rem,calc(100vw-1rem))]"
    >
      <p className="max-w-measure">{HMUL_EXPLANATION}</p>
    </HelpPopover>
  );
}
