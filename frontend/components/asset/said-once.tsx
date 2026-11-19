'use client';

import type { ReactNode } from 'react';
import { Dotted } from '@/components/ui/phone-row';
import { unitText } from '@/components/ui/units';
import { formatCount } from '@/lib/format-count';

/**
 * Whether every row of a listed page says the same in one column (readability P2; lane P2's
 * follow-up, item 3): two rows or more, each `read` alike (a value, or null for each). A page of one
 * row has nothing to fold, so a single row keeps its columns.
 */
export function onEveryRow<T>(rows: readonly T[], read: (row: T) => string | null): boolean {
  return rows.length > 1 && rows.every((row) => read(row) === read(rows[0]));
}

/**
 * What every row of a list says alike, said once over it, its columns dropped (lane P2's follow-up,
 * item 3; the review: twenty rows of "COPPER · 4,788", of the same owner, of ".bit Lock"): "every
 * one of the 20 holders listed · .bit Lock". The lead names the rows it speaks for (the page's, not
 * the list's whole: a page is what the reader sees), quiet as a label; each part is the value in
 * the reading ink, as its column wrote it. Nothing is drawn where no value is shared.
 */
export function SaidOnce({
  count,
  singular,
  plural,
  parts,
}: {
  /** The rows of the page it speaks for. */
  count: number;
  singular: string;
  plural?: string;
  /** Each shared value, with its key. */
  parts: readonly ReactNode[];
}) {
  if (parts.length === 0) return null;
  return (
    <p data-role="said-once" className="text-text text-small max-w-measure">
      <Dotted
        parts={[
          <span key="every" className="text-text-dim">
            {unitText(`every one of the ${formatCount(count, singular, plural)} listed`)}
          </span>,
          ...parts,
        ]}
      />
    </p>
  );
}
