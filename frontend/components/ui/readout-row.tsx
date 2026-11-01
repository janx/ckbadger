import type { ReactNode } from 'react';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';

export interface ReadoutRowProps {
  /** The accessible name of the readout (it is a polite live region). */
  label: string;
  /** Whether the reading is a pinned one (a tap): it is then introduced as "tapped". */
  pinned: boolean;
  /** What to say while nothing is hovered or pinned. */
  idle?: ReactNode;
  /** The reading of the hovered or pinned target; null when there is none. */
  children: ReactNode;
}

/**
 * The persistent line under a figure that a tap pins (plan R6, `usePinnedReadout`): the reading on
 * the left, introduced as "tapped" when pinned, and the shared hint "tap or hover" on the right.
 */
export function ReadoutRow({ label, pinned, idle, children }: ReadoutRowProps) {
  const empty = children === null || children === undefined || children === false;
  if (pinned && empty) throw new Error(`${label}: readout pinned with nothing to read`);
  return (
    <div
      role="status"
      aria-live="polite"
      aria-label={label}
      className="border-base-border-subtle flex items-baseline justify-between gap-3 border-t pt-2 text-[12px]"
    >
      <span className="text-text min-w-0" style={{ overflowWrap: 'anywhere' }}>
        {empty ? (
          <span className="text-text-dim">{idle}</span>
        ) : (
          <>
            {pinned && <span className="text-text-dim">tapped </span>}
            {children}
          </>
        )}
      </span>
      <TapOrHoverHint className="text-text-dim shrink-0 whitespace-nowrap text-[11px]" />
    </div>
  );
}
