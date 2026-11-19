'use client';

import { useState } from 'react';
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
 * words beside it, which a screen reader hears (`role="status"`): `copied` for 1.5 s after a copy
 * that worked; `not copied` where the clipboard refused the write or there is none (a page over
 * plain HTTP on a LAN address), until the next try. It never claims a copy that did not happen.
 *
 * Copy has one form (lane c2-finish, item 4): an id is text, and this icon after it copies it (a
 * head's id through HeadId's `copy`; a full id on its own line in tier 2 after the id). The one copy
 * that is an action with a purpose, the crash page's, writes its words after the icon (`words`).
 */
export function CopyButton({ value, label, words = false, className }: CopyButtonProps) {
  const { copied, copy } = useCopy(value);
  const [refused, setRefused] = useState(false);
  return (
    <span className={cn('inline-flex items-center gap-1.5', className)}>
      <button
        type="button"
        aria-label={words ? undefined : label}
        onClick={async () => setRefused(!(await copy()))}
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
      <span role="status" className={cn('text-meta', refused ? 'text-negative' : 'text-jade')}>
        {copied ? 'copied' : refused ? 'not copied' : null}
      </span>
    </span>
  );
}
