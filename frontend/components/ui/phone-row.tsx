import { createContext, useContext, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { unitNode } from '@/components/ui/units';

/**
 * The three voices of a phone row (boards L1–L8): the identity line, the line that says what the
 * row is, and the quiet developer figures last. The quiet line is quieter by its size, not its ink:
 * its figures are values, so they read in the reading ink (readability ruling R3); a label among
 * them is the call site's to set in the quiet ink, and the separators are quiet here.
 */
export type PhoneLineTone = 'head' | 'body' | 'quiet';

export interface PhoneLine {
  /** Names the line for tests and the gate (`data-line`). */
  key: string;
  tone: PhoneLineTone;
  left: ReactNode;
  /**
   * The figure the line ends with; it keeps the right edge, and drops under `left` whole. It is
   * never wider than the line: on a line of its own a part longer than the line takes the line's
   * width and breaks inside itself (a `Dotted` list between its figures, from the line's left
   * edge), so a long run of figures makes the row taller, never the page wider.
   */
  right?: ReactNode;
  /** The left part takes the free width (a bar). */
  grow?: boolean;
}

const TONE: Record<PhoneLineTone, string> = {
  head: 'text-body text-text-bright',
  body: 'text-small text-text',
  // An exact amount takes the ink of its line here (CkbAmount sets its own), so the developer
  // figures read in one ink, the reading ink, a step under the body line by size (R3); a part that
  // wraps itself in brighter ink keeps it.
  quiet: 'text-meta text-text [&_[data-ckb-amount]]:[color:inherit]',
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
  head,
  className,
  testId,
}: {
  lines: readonly (PhoneLine | null | false)[];
  mark?: ReactNode;
  /**
   * The first line's width as the list states it (`phoneHead`, lib/type-metrics.ts): the stone,
   * the key at its widest real value, the room between the parts and the widest figure at the
   * right. Written on the row (`data-head-px`), so a test reads what the row draws against what
   * its list claims.
   */
  head?: number;
  className?: string;
  testId?: string;
}) {
  const shown = lines.filter((line): line is PhoneLine => Boolean(line));
  if (shown.length < 1) throw new Error('A phone row needs at least one line');
  return (
    <div
      data-phone-row
      data-head-px={head}
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
          // gap-x-3: PHONE_PART_GAP_PX, the 12 px a list's stated first line counts between its
          // parts (lib/type-metrics.ts; phone-row.test reads the class against it)
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
            {unitNode(line.left)}
          </span>
          {line.right !== undefined && line.right !== null && (
            <span
              data-part="right"
              className={cn(
                'ml-auto max-w-full shrink-0 text-right tabular-nums',
                line.tone === 'head' && 'text-text text-small'
              )}
            >
              {unitNode(line.right)}
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
  /** A navigation landmark of the run's links (named by `label`) in place of a plain span. */
  readonly nav?: { label: string };
}

/**
 * Parts of a separated list that belong together (an activity head's protocol facts, its type
 * calls): one element names them (`data-role`) and sets their voice, but it draws no box
 * (`display: contents`), so each part is an item of the one list and wraps and separates as any.
 */
export function dottedGroup(
  parts: readonly ReactNode[],
  { role, className, nav }: { role: string; className?: string; nav?: { label: string } }
): DottedGroup {
  return { [GROUP]: true, parts, role, className, nav };
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
 * The geometry, in CSS alone: the list is a wrapping flex row that clips what lies LEFT of its box
 * and nothing else (clip-path inset(-1em -100vw -1em -0.75ch)): never its right, so a unit wider
 * than the list shows past its edge, where the page-overflow check sees it, instead of being cut
 * with no scroll and no error (overflow-hidden cut it). `min-w-0` lets it shrink inside a flex
 * row. The left edge stands 0.75ch out: a hung "· " has its dot in the cell that ends 1ch left of
 * the box, still clipped, while the first item's focus ring (1 px at a 2 px offset, 3 px out;
 * 0.75ch is 4.95 px at 11 px, 5.85 px at 13 px) is not cut.
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
 *
 * `inline`: the list stands in running text after other words (the 404 seal's caption), one box
 * on its line (`inline-flex`): where it does not fit what is left of the line it moves to the next
 * line whole, so the words before it break before it, never between its parts; only a line
 * narrower than the whole list wraps it inside, by the rule above.
 */
export function Dotted({
  parts,
  className,
  oneLine = false,
  inline = false,
}: {
  parts: readonly (ReactNode | DottedGroup)[];
  className?: string;
  oneLine?: boolean;
  inline?: boolean;
}) {
  if (useContext(InDotted))
    throw new Error('a separated list inside another: pass its parts to the outer list');
  let index = 0;
  const item = (part: ReactNode) => {
    const i = index++;
    if (oneLine)
      return (
        <span key={i} data-dotted-item="">
          {/* a separator: quiet by rule (R3) */}
          {i > 0 && (
            <span data-dotted-separator="" aria-hidden="true" className="text-text-dim">
              {' · '}
            </span>
          )}
          {unitNode(part)}
        </span>
      );
    return i === 0 ? (
      <span key={i} data-dotted-item="" className="min-w-0">
        {unitNode(part)}
      </span>
    ) : (
      <span key={i} data-dotted-item="" className="-ml-[2ch] min-w-0 pl-[2ch] -indent-[2ch]">
        {/* a separator: quiet by rule (R3) */}
        <span
          data-dotted-separator=""
          aria-hidden="true"
          className="text-text-dim whitespace-nowrap"
        >
          {' · '}
        </span>
        <span className="indent-0">{unitNode(part)}</span>
      </span>
    );
  };
  const nodes = parts.flatMap((part, g) => {
    if (!isGroup(part)) return shownPart(part) ? [item(part)] : [];
    const shown = part.parts.filter(shownPart);
    if (shown.length === 0) return [];
    const Group = part.nav ? 'nav' : 'span';
    return [
      <Group
        key={`g${g}`}
        data-role={part.role}
        aria-label={part.nav?.label}
        className={cn('contents', part.className)}
      >
        {shown.map(item)}
      </Group>,
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
          inline ? 'inline-flex' : 'flex',
          'min-w-0 flex-wrap items-baseline gap-x-[3ch] [clip-path:inset(-1em_-100vw_-1em_-0.75ch)]',
          className
        )}
      >
        {nodes}
      </span>
    </InDotted.Provider>
  );
}
