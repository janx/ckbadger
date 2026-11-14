import type { ComponentProps } from 'react';
import { cn } from '@/lib/utils';

/**
 * One unit that a line never breaks inside (`docs/RESPONSIVE_LAYOUT.md`, "Non-breaking units"): a
 * shortened hash at its ellipsis, a date at its hyphens, a date from its time, a number from its
 * unit, a label from its address. The line breaks before the unit or after it. This is the one
 * piece that does it: the renderers of each kind of token (HexCode, Address, OutpointIndex,
 * Utc8Date) use it, and a site that prints such a token as a bare string wraps it here, never in a
 * class of its own.
 *
 * `data-nobreak` names it for tests and the gate.
 */
export function NoBreak({ className, ...props }: ComponentProps<'span'>) {
  return <span data-nobreak="" className={cn('whitespace-nowrap', className)} {...props} />;
}
