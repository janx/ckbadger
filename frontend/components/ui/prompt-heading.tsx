import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface PromptHeadingProps {
  /** The command: the section's name, in the moon ink, set lowercase. */
  command: ReactNode;
  /** What the command is run on (`--tip 20,969,560`, `3 in → 2 out`): dim, regular, its own case. */
  args?: ReactNode;
  /** The right prompt: a quiet note at the row's right end, outside the heading's name. */
  note?: ReactNode;
  /**
   * An h2 is a tier-1 section heading (`text-section`), whose square is its own `::before`
   * (globals.css, the rule every hand-written tier-1 h2 shares). Any other element draws the
   * square as a `.section-mark` — or as `mark`, for a frame whose square is also its indicator.
   */
  as?: 'h2' | 'h3' | 'div';
  /** The square for a heading that is not an h2, in place of the plain `.section-mark`. */
  mark?: ReactNode;
  id?: string;
  className?: string;
}

/**
 * A section as a command line (rubbing × Song print, phase 2): the 8 px cinnabar prompt square, the
 * command in lowercase, its args in the dim ink, and an optional right-prompt note. The heading's
 * accessible name is the command and its args; the note stands beside the heading, not inside it.
 */
export function PromptHeading({
  command,
  args,
  note,
  as: Tag = 'h2',
  mark,
  id,
  className,
}: PromptHeadingProps) {
  if (Tag === 'h2' && mark !== undefined)
    throw new Error('PromptHeading: an h2 draws its square in CSS (h2.text-section::before)');
  const heading = (
    <Tag
      id={id}
      className={cn(
        'prompt text-text-bright flex min-w-0 flex-wrap items-baseline',
        Tag === 'h2' && 'text-section',
        className
      )}
    >
      {Tag !== 'h2' && (mark ?? <span aria-hidden="true" className="section-mark mr-2.5" />)}
      <span className="lowercase">{command}</span>
      {args !== undefined && (
        <span className="text-text-dim ml-2.5 font-normal normal-case">{args}</span>
      )}
    </Tag>
  );
  if (note === undefined) return heading;
  return (
    <div className="flex min-w-0 flex-wrap items-baseline gap-x-4 gap-y-1">
      {heading}
      <span data-prompt-note="" className="text-text-ghost text-meta ml-auto">
        {note}
      </span>
    </div>
  );
}
