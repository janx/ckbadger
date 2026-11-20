import type { ReactNode } from 'react';
import { Dotted } from '@/components/ui/phone-row';
import { color, type PaletteKey } from '@/lib/palette';
import { cn } from '@/lib/utils';

/** A strip holds at most this many facts (the transaction board: status, time, block, fee). */
const MOST_FACTS = 4;

/** One fact of a status strip. */
export interface StripFact {
  /** The fact's name: a label, lowercase as the board writes it ("status", "time", "block"). */
  label: string;
  /**
   * The value, in the strip's bright voice: one unit or several, each held whole by the caller
   * (a date-time, a count with its noun, a shortened id are units already). Never empty: a fact
   * the page does not know is not passed.
   */
  value: ReactNode;
  /** What the value's note says ("36 confirmations", "4m ago"), each part after a separator. */
  note?: ReactNode | readonly ReactNode[];
  /** A state's dot before the value (committed, live, canonical), in this ink. */
  dot?: PaletteKey;
}

const shown = (node: ReactNode) =>
  node !== null && node !== undefined && node !== false && node !== '';

/**
 * The status strip (lane D2; the UI review's PA-13 and R10, the transaction board passed at gate
 * D0′): the facts a reader checks first about the object a page is of (its state, its time, where
 * it sits, its one figure), under the id line of the five entity pages (transaction, block, cell,
 * address, uncle), in one placement at every width. A `dl` of at most four facts between two row
 * rules, named by `label` as a group.
 *
 * From sm the facts stand in one line, 44 px apart: each a column, its label (12 px, the quiet
 * ink) 4 px over its value (14 px, the bright ink), the value's note 13 px in the quiet ink after
 * the one separator; nothing wraps inside a fact. Where the line cannot hold every fact a fact
 * moves to a second row whole (a transaction's four are 1,108.6 px at 1280's 1,232 and take two
 * rows at 1024's 976). Below sm each fact is one labelled line: the labels' column, 12 px, then the
 * value at 13 px, which wraps only between its parts — the value and each note, each held whole —
 * and never starts a line with a separator (`Dotted`, the one separator-joined list).
 *
 * Inks by role (readability R3): a label is quiet, a value bright, a note quiet. The strip states
 * what it is given and nothing else; it refuses a fifth fact, an empty value, no facts and two
 * facts of one name, naming the strip.
 */
export interface StatusStripProps {
  /** What the strip is the status of, its name as a group ("Transaction status"). */
  label: string;
  facts: readonly StripFact[];
  /** Where it stands: its margins (the head places it). */
  className?: string;
}

export function StatusStrip({ label, facts, className }: StatusStripProps) {
  if (facts.length === 0) throw new Error(`status strip "${label}": no facts`);
  if (facts.length > MOST_FACTS)
    throw new Error(`status strip "${label}": ${facts.length} facts; a strip holds at most four`);
  const names = new Set<string>();
  for (const fact of facts) {
    if (names.has(fact.label))
      throw new Error(`status strip "${label}": two facts named "${fact.label}"`);
    names.add(fact.label);
    if (!shown(fact.value))
      throw new Error(
        `status strip "${label}": the fact "${fact.label}" has no value; a fact the page does not know is not passed`
      );
  }
  return (
    <dl
      role="group"
      aria-label={label}
      data-status-strip=""
      className={cn(
        'border-row-rule grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-baseline gap-x-3 gap-y-1 border-y py-2.5 sm:flex sm:flex-wrap sm:gap-x-11 sm:gap-y-3 sm:py-3',
        className
      )}
    >
      {facts.map((fact) => {
        const notes = (Array.isArray(fact.note) ? fact.note : [fact.note]).filter(shown);
        return (
          <div
            key={fact.label}
            data-strip-fact={fact.label}
            className="contents sm:flex sm:min-w-0 sm:flex-col sm:gap-1"
          >
            {/* a fact's name: a label, quiet (R3) */}
            <dt className="text-text-dim text-meta">{fact.label}</dt>
            <dd className="text-text-bright text-small sm:text-body min-w-0">
              <Dotted
                className="sm:flex-nowrap"
                parts={[
                  fact.dot ? (
                    // the dot is held to its word: one box, the dot centred on the word's line
                    <span key="value" className="inline-flex items-baseline">
                      <span
                        data-strip-dot=""
                        aria-hidden="true"
                        className="mr-[7px] inline-block h-1.5 w-1.5 shrink-0 self-center rounded-full"
                        style={{ backgroundColor: color(fact.dot) }}
                      />
                      {fact.value}
                    </span>
                  ) : (
                    <span key="value">{fact.value}</span>
                  ),
                  // a note: quiet, a step under the value (R3)
                  ...notes.map((note, i) => (
                    <span key={`note-${i}`} className="text-text-dim text-small">
                      {note}
                    </span>
                  )),
                ]}
              />
            </dd>
          </div>
        );
      })}
    </dl>
  );
}
