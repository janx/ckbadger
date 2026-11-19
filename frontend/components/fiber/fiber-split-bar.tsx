'use client';

import type { ReactNode } from 'react';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { FIBER_COLORS } from '@/lib/design-tokens';
import { unitNode } from '@/components/ui/units';

interface Side {
  name: ReactNode;
  shannons: bigint;
}

/**
 * One channel split as a single bar (FiberChannel board V66, "What moved inside the channel"):
 * the first party grey, the second jade, widths from the two exact amounts.
 */
export function FiberSplitBar({ left, right, note }: { left: Side; right: Side; note: ReactNode }) {
  if (left.shannons <= BigInt(0) || right.shannons <= BigInt(0))
    throw new Error(
      `Fiber split bar needs two positive amounts, got ${left.shannons} and ${right.shannons}`
    );
  const total = left.shannons + right.shannons;
  // Percent with 4 decimals: a presentation width, the amounts themselves stay exact.
  const leftPercent = Number((left.shannons * BigInt(1_000_000)) / total) / 10_000;
  return (
    <div data-split-bar className="min-w-0">
      {/* A figure's labels are its values (lane P4): each party's id in the reading ink and its
          amount exact, read out as the figure draws it (no padding to eight decimals: the
          column form is a table column's); the note under the bar is the figure's note. */}
      <div className="text-body mb-1.5 flex flex-wrap justify-between gap-4">
        <span>
          <span className="text-text">{left.name}</span>{' '}
          <CkbAmount form="exact" shannons={left.shannons} signed={false} tone="plain" />
        </span>
        <span>
          <CkbAmount form="exact" shannons={right.shannons} signed={false} tone="plain" />{' '}
          <span className="text-text">{right.name}</span>
        </span>
      </div>
      <div className="flex h-[22px] gap-0.5" aria-hidden="true">
        <div
          data-split="left"
          style={{ width: `${leftPercent}%`, background: FIBER_COLORS.historyFill }}
        />
        <div data-split="right" className="flex-1" style={{ background: FIBER_COLORS.openFill }} />
      </div>
      <div className="text-text-dim text-meta mt-1.5">{unitNode(note)}</div>
    </div>
  );
}
