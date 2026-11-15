import type { ComponentProps } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { shortenId, type IdKind, type ShortSize } from '@/lib/shorten';

/**
 * A shortened id written as text (U-09): the hash or address at one of the three sizes of
 * `lib/shorten.ts`, one unit a line never breaks inside (`NoBreak`). It draws no ink, link, copy or
 * title of its own, so it stands inside what the call site draws: a link with its own look and
 * the full value in its title, a row's key, a sentence. `HexCode` (a copyable hex value) and
 * `Address` (the address link) shorten by the same rule; this is the piece for the text alone.
 */
export function ShortId({
  value,
  kind,
  size,
  ...props
}: Omit<ComponentProps<'span'>, 'children'> & { value: string; kind: IdKind; size: ShortSize }) {
  return <NoBreak {...props}>{shortenId(value, kind, size)}</NoBreak>;
}
