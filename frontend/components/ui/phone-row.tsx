import { createContext, useContext, type ReactNode } from 'react';
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
 *
 * `mark` (the row seal, rubbing × Song print phase 3) leads the row as its own column, top-aligned
 * beside every line, so each line keeps one left edge after it (the Transaction board's cell rows);
 * the lines stay the row's direct children.
 */
export function PhoneRow({
  lines,
  mark,
  className,
  testId,
}: {
  lines: readonly (PhoneLine | null | false)[];
  mark?: ReactNode;
  className?: string;
  testId?: string;
}) {
  const shown = lines.filter((line): line is PhoneLine => Boolean(line));
  if (shown.length < 1) throw new Error('A phone row needs at least one line');
  return (
    <div
      data-phone-row
      data-testid={testId}
      className={cn(
        mark
          ? 'grid min-w-0 grid-cols-[auto_minmax(0,1fr)] gap-y-[3px]'
          : 'flex min-w-0 flex-col gap-[3px]',
        className
      )}
    >
      {mark && (
        <span
          data-part="mark"
          className="col-start-1 self-start"
          style={{ gridRow: `1 / span ${shown.length}` }}
        >
          {mark}
        </span>
      )}
      {shown.map((line) => (
        <div
          key={line.key}
          data-line={line.key}
          data-tone={line.tone}
          className={cn(
            'flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5',
            mark && 'col-start-2',
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

/** Set inside a separated list: a list inside another would break the hanging geometry. */
const InDotted = createContext(false);

const GROUP = Symbol('dotted group');

/** A run of a list's parts that its page names as one thing (`data-role`), laid out as items. */
export interface DottedGroup {
  readonly [GROUP]: true;
  readonly parts: readonly ReactNode[];
  readonly role: string;
  readonly className?: string;
}

/**
 * Parts of a separated list that belong together (an activity head's protocol facts, its type
 * calls): one element names them (`data-role`) and sets their voice, but it draws no box
 * (`display: contents`), so each part is an item of the one list and wraps and separates as any.
 */
export function dottedGroup(
  parts: readonly ReactNode[],
  { role, className }: { role: string; className?: string }
): DottedGroup {
  return { [GROUP]: true, parts, role, className };
}

function isGroup(part: ReactNode | DottedGroup): part is DottedGroup {
  return typeof part === 'object' && part !== null && GROUP in part;
}

const shownPart = (part: ReactNode) =>
  part !== null && part !== undefined && part !== false && part !== '';

/**
 * The one separator-joined list (PA-25, PB-23): parts joined by a quiet middle dot, empty parts
 * left out, so no separator is ever left with nothing after it. A separator belongs to the item
 * after it: a line breaks before the dot, never after it, and a dot is not drawn when its item
 * starts a line.
 *
 * The geometry, in CSS alone: the list is a wrapping flex row that clips what lies left of its box.
 * Every item after the first hangs its " · " 2ch left of its own text (`pl-[2ch] -indent-[2ch]`)
 * and pulls its box 2ch left (`-ml-[2ch]`). Mid-line the 3ch gap less those 2ch leaves 1ch before
 * the dot ("a · b"); at a line's start the dot lies in the 2ch left of the list's box and is
 * clipped, and the item's text starts at the list's edge. An item too long for a line wraps inside
 * itself, its further lines at its padding edge, the list's edge. What an item holds is reset to
 * no indent (`indent-0`), as text-indent is inherited.
 *
 * The parts are flat: a list inside another refuses to render (pass its parts to the outer list;
 * a run the page names as one thing is a `dottedGroup`). The text reads "a · b · c" (the dots are
 * hidden from assistive technology, which reads the parts as a list of phrases). `className` sets
 * the list's own voice (size, ink); the gap is 3ch of it.
 *
 * `oneLine`: the same parts and separators as inline text, for a line that never wraps (a table
 * cell that ends in an ellipsis): with no line to start, no separator can open or end one.
 */
export function Dotted({
  parts,
  className,
  oneLine = false,
}: {
  parts: readonly (ReactNode | DottedGroup)[];
  className?: string;
  oneLine?: boolean;
}) {
  if (useContext(InDotted))
    throw new Error('a separated list inside another: pass its parts to the outer list');
  let index = 0;
  const item = (part: ReactNode) => {
    const i = index++;
    if (oneLine)
      return (
        <span key={i} data-dotted-item="">
          {i > 0 && (
            <span data-dotted-separator="" aria-hidden="true" className="text-text-dim">
              {' · '}
            </span>
          )}
          {part}
        </span>
      );
    return i === 0 ? (
      <span key={i} data-dotted-item="" className="min-w-0">
        {part}
      </span>
    ) : (
      <span key={i} data-dotted-item="" className="-ml-[2ch] min-w-0 pl-[2ch] -indent-[2ch]">
        <span
          data-dotted-separator=""
          aria-hidden="true"
          className="text-text-dim whitespace-nowrap"
        >
          {' · '}
        </span>
        <span className="indent-0">{part}</span>
      </span>
    );
  };
  const nodes = parts.flatMap((part, g) => {
    if (!isGroup(part)) return shownPart(part) ? [item(part)] : [];
    const shown = part.parts.filter(shownPart);
    if (shown.length === 0) return [];
    return [
      <span key={`g${g}`} data-role={part.role} className={cn('contents', part.className)}>
        {shown.map(item)}
      </span>,
    ];
  });
  if (nodes.length === 0) return null;
  if (oneLine)
    return (
      <span data-dotted="" className={cn('whitespace-nowrap', className)}>
        {nodes}
      </span>
    );
  return (
    <InDotted.Provider value={true}>
      <span
        data-dotted=""
        className={cn(
          'flex min-w-0 flex-wrap items-baseline gap-x-[3ch] overflow-hidden',
          className
        )}
      >
        {nodes}
      </span>
    </InDotted.Provider>
  );
}
