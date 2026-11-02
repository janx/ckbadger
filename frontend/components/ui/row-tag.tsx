import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * `default`: a role, symbol, standard or lock kind. `ghost`: what is set back ("deprecated").
 * `accent`: a class mark ("BTC+CKB", "Pure CKB", "live · current").
 */
export type RowTagVariant = 'default' | 'ghost' | 'accent';

/** The boards' tag (table-rows T5): 11 px, 16 px line, 1 px border, 3 px corners, 5 px sides. */
const BASE =
  'text-meta text-text-dim border-base-border inline-block whitespace-nowrap rounded-[3px] border px-[5px] align-middle leading-4';
const VARIANT: Record<RowTagVariant, string> = {
  default: '',
  ghost: 'border-dashed text-[#4b5266]',
  accent: 'text-jade border-[#1f5a46]',
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
