'use client';

import type { MouseEvent, Ref } from 'react';
import { color } from '@/lib/palette';

interface HelpMarkProps {
  /** Its name for a screen reader (`Explain HMul`). */
  label: string;
  /** Whether what it opens is open. */
  expanded: boolean;
  /** The id of what it opens. */
  controls: string;
  onClick: (event: MouseEvent<HTMLButtonElement>) => void;
  ref?: Ref<HTMLButtonElement>;
}

/**
 * The help mark (the controls board, gate D0′; UI review 2026-11-14, phase C): one mark, an 18 px
 * square box on the quieter frame line (the board's second line, `slate-other`) so its 12 px `?`
 * in the quiet ink has room; its target reaches 7 px past each edge, 32 px at every width, while
 * the box stays 18 px in its line. It is the trigger of the one help popover: HelpPopover
 * (components/ui/help-popover.tsx) draws it and keeps everything it opens — the panel, its hover,
 * focus, pin and Escape, its place by the mark. A page writes HelpPopover; the mark is never
 * drawn alone, and there is no second popover.
 */
export function HelpMark({ label, expanded, controls, onClick, ref }: HelpMarkProps) {
  return (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      aria-expanded={expanded}
      aria-controls={controls}
      onClick={onClick}
      style={{ borderColor: color('slate-other') }}
      className="text-meta text-text-dim hover:text-jade relative inline-flex h-[18px] w-[18px] flex-none items-center justify-center border font-mono transition-colors before:absolute before:-inset-[7px] before:content-['']"
    >
      ?
    </button>
  );
}
