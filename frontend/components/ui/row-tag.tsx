import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * `default`: a role, symbol, standard or lock kind. `ghost`: what is set back or said beside the
 * row's own party ("this address", the network table's "Not dialed by this crawler") — the dashed
 * border sets it apart, the ink is the readable dim (lane F follow-up: the faint #4b5266 could not
 * be read). `accent`: a class mark ("BTC+CKB", "Pure CKB", "live · current", "immutable").
 * `warn`: a status the reader must act on — "deprecated", a script no new cell should use. Red is
 * otherwise reserved for negative deltas (CKB and tokens leaving a party); this is the one status
 * that earns it, because a user must not pick this script (lane F follow-up 2: the dim tag read as
 * a footnote and warned no one). Rouge ink on a solid rouge border.
 */
export type RowTagVariant = 'default' | 'ghost' | 'accent' | 'warn';

/** The boards' tag (table-rows T5): 11 px, 16 px line, 1 px border, 2 px corners, 5 px sides. */
const BASE =
  'text-meta text-text-dim border-base-border inline-block whitespace-nowrap rounded-sm border px-[5px] align-middle leading-4';
const VARIANT: Record<RowTagVariant, string> = {
  default: '',
  ghost: 'border-dashed',
  accent: 'text-jade border-[#1f5a46]',
  warn: 'text-rouge border-rouge',
};

/** The class string of a row tag, for a caller that draws the chip on its own element. */
export function rowTagClass(variant: RowTagVariant = 'default', className?: string): string {
  return cn(BASE, VARIANT[variant], className);
}

export interface RowTagProps {
  children: ReactNode;
  variant?: RowTagVariant;
  title?: string;
  className?: string;
}

/** A row's 11 px bordered chip beside its bright key (table-rows R3). */
export function RowTag({ children, variant = 'default', title, className }: RowTagProps) {
  if (children === null || children === undefined || children === false || children === '')
    throw new Error('RowTag: empty tag');
  return (
    <span data-row-tag={variant} title={title} className={rowTagClass(variant, className)}>
      {children}
    </span>
  );
}
