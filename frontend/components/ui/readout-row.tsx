'use client';

import type { ReactNode } from 'react';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';
import { usePinWord } from '@/hooks/usePinWord';
import { cn } from '@/lib/utils';

export interface ReadoutRowProps {
  /** The accessible name of the readout (it is a polite live region). */
  label: string;
  /**
   * Whether the reading is a pinned one: it is then introduced as "selected" — "tapped" on a
   * touch screen (consistency C11, `usePinWord`).
   */
  pinned: boolean;
  /** What to say while nothing is hovered or pinned. */
  idle?: ReactNode;
  /**
   * Whether the row carries the section's one "tap or hover" (consistency C10). Default true; a
   * section of several figures states the hint once by its heading and passes false here.
   */
  hint?: boolean;
  /** The reading of the hovered or pinned target; null when there is none. */
  children: ReactNode;
}

/**
 * The persistent line under a figure that a tap pins (plan R6, `usePinnedReadout`): the reading on
 * the left, introduced as "selected" (or "tapped") when pinned, and the shared hint "tap or hover"
 * on the right — the section's one hint (C10).
 */
export function ReadoutRow({ label, pinned, idle, hint = true, children }: ReadoutRowProps) {
  const pinWord = usePinWord();
  const empty = children === null || children === undefined || children === false;
  if (pinned && empty) throw new Error(`${label}: readout pinned with nothing to read`);
  // Hover never moves layout: with no idle words the row still holds its text line (a no-break
  // space), so the first hover changes the words in the line, not the page's height below it.
  const idleWords =
    idle === null || idle === undefined || idle === false || idle === '' ? '\u00a0' : idle;
  const ghost = typeof idle === 'string' && idle !== '' ? idle : undefined;
  return (
    <div
      role="status"
      aria-live="polite"
      aria-label={label}
      className="border-base-border-subtle text-small flex items-baseline justify-between gap-3 border-t pt-2"
    >
      {/* Hover never moves layout: an idle sentence stays reserved under the reading as an
          invisible ghost in the same grid cell (generated content, so it is neither the reading's
          text nor its accessible name) — a one-line reading cannot shrink a row whose idle words
          wrap to two lines on a phone. */}
      <span
        data-idle={ghost}
        className={cn(
          'text-text grid min-w-0',
          ghost !== undefined &&
            'after:invisible after:content-[attr(data-idle)] after:[grid-area:1/1]'
        )}
        style={{ overflowWrap: 'anywhere' }}
      >
        <span className="[grid-area:1/1]">
          {empty ? (
            <span className="text-text-dim">{idleWords}</span>
          ) : (
            <>
              {pinned && <span className="text-text-dim">{pinWord} </span>}
              {children}
            </>
          )}
        </span>
      </span>
      {hint && <TapOrHoverHint className="text-text-dim text-meta shrink-0 whitespace-nowrap" />}
    </div>
  );
}
