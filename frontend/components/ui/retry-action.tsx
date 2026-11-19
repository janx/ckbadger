'use client';

import { linkLookClass } from '@/components/ui/link';
import { cn } from '@/lib/utils';

interface RetryActionProps {
  /** Its words, the caller's: `Retry`. */
  words: string;
  /** Read again: the failed read's refetch, or its next page's fetch. */
  onRetry: () => void;
  className?: string;
}

/**
 * The one form of an action that follows a failed read (lane cd-fixes-controls, the chart
 * reviewer's N11 handed to the library: "Retry" was a raw button in four places, each with its own
 * ink and underline). A real button, its words the caller's; its ink the link rule's way onward
 * (components/ui/link.tsx, the `onward` look: celadon), no underline, and under the pointer and the
 * keyboard's focus the rule every link follows (app/globals.css, `.look-action`); 28 px high from
 * 640 and 32 below it, like the other parts of a control. It reads at its line's step, as a link
 * does. The controls gate counts a retry written by hand outside it.
 */
export function RetryAction({ words, onRetry, className }: RetryActionProps) {
  if (words.trim() === '') throw new Error('RetryAction: an action has its words');
  return (
    <button
      type="button"
      onClick={onRetry}
      className={cn(
        linkLookClass('onward'),
        'look-action inline-flex h-8 items-center whitespace-nowrap sm:h-7',
        className
      )}
    >
      {words}
    </button>
  );
}
