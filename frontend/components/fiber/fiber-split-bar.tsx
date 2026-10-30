'use client';

import type { ReactNode } from 'react';
import { ExactCkb } from '@/components/activity-event-row';
import { FIBER_COLORS } from '@/lib/design-tokens';

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
    <div className="min-w-0">
      <div className="mb-1.5 flex flex-wrap justify-between gap-4 text-[12.5px]">
        <span>
          <span className="text-text-dim">{left.name}</span>{' '}
          <ExactCkb shannons={left.shannons} signed={false} tone="plain" />
        </span>
        <span>
          <ExactCkb shannons={right.shannons} signed={false} tone="plain" />{' '}
          <span className="text-text-dim">{right.name}</span>
        </span>
      </div>
      <div className="flex h-[22px] gap-0.5" aria-hidden="true">
        <div
          data-split="left"
          className="rounded-l"
          style={{ width: `${leftPercent}%`, background: FIBER_COLORS.historyFill }}
        />
        <div
          data-split="right"
          className="flex-1 rounded-r"
          style={{ background: FIBER_COLORS.openFill }}
        />
      </div>
      <div className="text-text-dim mt-1.5 text-[11.5px]">{note}</div>
    </div>
  );
}
