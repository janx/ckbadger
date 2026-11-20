'use client';

import { useLayoutEffect, useRef, useState } from 'react';
import { useCopy } from '@/hooks/useCopy';
import { cn } from '@/lib/utils';

interface CopyButtonProps {
  /** What it copies, whole. */
  value: string;
  /** Its name for a screen reader: what it copies (`copy transaction hash`). */
  label: string;
  /**
   * The label written after the icon, on screen (lane c2-finish, item 4): for a copy that is an
   * action with a purpose rather than an id's, the crash page's `copy details for a bug report`.
   */
  words?: boolean;
  className?: string;
}

/**
 * The copy button (the controls board, gate D0′; UI review 2026-11-14, phase C): one icon, the
 * board's two square sheets, in a 24 px box from 640 and a 32 px one below it (a thumb's), after
 * an id line, in the quiet ink. It copies through the one copy path (hooks/useCopy) and answers in
 * words, which a screen reader hears (`role="status"`), for 1.5 s: `copied` after a copy that
 * worked; `not copied` where the clipboard refused the write or there is none (a page over plain
 * HTTP on a LAN address). It never claims a copy that did not happen.
 *
 * Its answer moves no line (lane c2-finish, rulings round 1): a line that jumps under the reader's
 * finger is worse than the answer it carries, so the words stand out of the line's flow, a small
 * label under the box, its right edge on the box's right edge, so it never runs past the line's end
 * and never covers the id before the box; on the elevated ground in the frame line (`base-elevated`,
 * `base-border`), so the words under it do not show through. It takes no width and no height. Where
 * that edge would start the label before the page's left edge (a box at the start of a phone's
 * line: its right edge at 48 px, the label up to 86 px wide), it stands on the box's left edge
 * instead, measured when it appears. With nothing to say it draws nothing.
 *
 * Copy has one form (lane c2-finish, item 4): an id is text, and this icon after it copies it (a
 * head's id through HeadId's `copy`; a full id on its own line in tier 2 after the id). The one copy
 * that is an action with a purpose, the crash page's, writes its words after the icon (`words`).
 */
export function CopyButton({ value, label, words = false, className }: CopyButtonProps) {
  const { copied, refused, copy } = useCopy(value);
  const answer = copied ? 'copied' : refused ? 'not copied' : null;
  const answerRef = useRef<HTMLSpanElement>(null);
  const [edge, setEdge] = useState<'right' | 'left'>('right');
  useLayoutEffect(() => {
    if (answer === null) {
      setEdge('right');
      return;
    }
    const el = answerRef.current;
    if (el && edge === 'right' && el.getBoundingClientRect().left < 0) setEdge('left');
  }, [answer, edge]);
  return (
    <span className={cn('relative inline-flex items-start', className)}>
      <button
        type="button"
        aria-label={words ? undefined : label}
        onClick={copy}
        className={cn(
          'text-text-dim hover:text-jade inline-flex h-8 flex-none items-center transition-colors sm:h-6',
          words ? 'gap-1.5 text-left' : 'w-8 justify-center sm:w-6'
        )}
      >
        <svg
          aria-hidden="true"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
        >
          <rect x="8" y="8" width="13" height="13" />
          <path d="M4 16V3h13" />
        </svg>
        {words && <span className="whitespace-nowrap">{label}</span>}
      </button>
      <span
        ref={answerRef}
        role="status"
        className={cn(
          'text-meta pointer-events-none absolute top-full z-10 mt-1 whitespace-nowrap',
          edge === 'right' ? 'right-0' : 'left-0',
          answer !== null && 'bg-base-elevated border-base-border border px-1.5',
          refused ? 'text-negative' : 'text-jade'
        )}
      >
        {answer}
      </span>
    </span>
  );
}
