import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * The three voices of a phone row (boards L1–L8): the identity line, the line that says what the
 * row is, and the quiet developer figures last.
 */
export type PhoneLineTone = 'head' | 'body' | 'quiet';

export interface PhoneLine {
  /** Names the line for tests and the gate (`data-line`). */
  key: string;
  tone: PhoneLineTone;
  left: ReactNode;
  /** The figure the line ends with; it keeps the right edge, and drops under `left` whole. */
  right?: ReactNode;
  /** The left part takes the free width (a bar). */
  grow?: boolean;
}

const TONE: Record<PhoneLineTone, string> = {
  head: 'text-body text-text-bright',
  body: 'text-small text-text',
  // An exact amount takes the ink of its line here (ExactCkb sets its own), so the developer
  // figures stay quiet; a part that wraps itself in brighter ink keeps it.
  quiet: 'text-meta text-text-dim [&_[data-ckb-amount]]:[color:inherit]',
};

/**
 * One designed phone row of a list (R7): two or three lines, each a left part that wraps (never
 * truncated) and an optional right part. Every column of the list's table is on one of the lines.
 */
export function PhoneRow({
  lines,
  className,
  testId,
}: {
  lines: readonly (PhoneLine | null | false)[];
  className?: string;
  testId?: string;
}) {
  const shown = lines.filter((line): line is PhoneLine => Boolean(line));
  if (shown.length < 1) throw new Error('A phone row needs at least one line');
  return (
    <div
      data-phone-row
      data-testid={testId}
      className={cn('flex min-w-0 flex-col gap-[3px]', className)}
    >
      {shown.map((line) => (
        <div
          key={line.key}
          data-line={line.key}
          data-tone={line.tone}
          className={cn(
            'flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5',
            TONE[line.tone]
          )}
        >
          <span
            data-part="left"
            className={cn(
              'min-w-0 [overflow-wrap:anywhere]',
              line.grow && 'flex flex-1 items-center'
            )}
          >
            {line.left}
          </span>
          {line.right !== undefined && line.right !== null && (
            <span
              data-part="right"
              className={cn(
                'ml-auto shrink-0 text-right tabular-nums',
                line.tone === 'head' && 'text-text text-small'
              )}
            >
              {line.right}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

/** Parts of a line joined by a quiet middle dot, empty parts left out. */
export function Dotted({ parts }: { parts: readonly ReactNode[] }) {
  const shown = parts.filter((part) => part !== null && part !== undefined && part !== false);
  return (
    <>
      {shown.map((part, i) => (
        <span key={i}>
          {i > 0 && <span className="text-text-dim"> · </span>}
          {part}
        </span>
      ))}
    </>
  );
}
