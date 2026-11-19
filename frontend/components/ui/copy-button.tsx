'use client';

import { useState } from 'react';
import { useCopy } from '@/hooks/useCopy';
import { cn } from '@/lib/utils';

interface CopyButtonProps {
  /** What it copies, whole. */
  value: string;
  /** Its name for a screen reader: what it copies (`copy transaction hash`). */
  label: string;
  className?: string;
}

/**
 * The copy button (the controls board, gate D0′; UI review 2026-11-14, phase C): one icon, the
 * board's two square sheets, in a 24 px box from 640 and a 32 px one below it (a thumb's), after
 * an id line, in the quiet ink. It copies through the one copy path (hooks/useCopy) and answers in
 * words beside it, which a screen reader hears (`role="status"`): `copied` for 1.5 s after a copy
 * that worked; `not copied` where the clipboard refused the write or there is none (a page over
 * plain HTTP on a LAN address), until the next try. It never claims a copy that did not happen.
 */
export function CopyButton({ value, label, className }: CopyButtonProps) {
  const { copied, copy } = useCopy(value);
  const [refused, setRefused] = useState(false);
  return (
    <span className={cn('inline-flex items-center gap-1.5', className)}>
      <button
        type="button"
        aria-label={label}
        onClick={async () => setRefused(!(await copy()))}
        className="text-text-dim hover:text-jade inline-flex h-8 w-8 flex-none items-center justify-center transition-colors sm:h-6 sm:w-6"
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
      </button>
      <span role="status" className={cn('text-meta', refused ? 'text-negative' : 'text-jade')}>
        {copied ? 'copied' : refused ? 'not copied' : null}
      </span>
    </span>
  );
}
