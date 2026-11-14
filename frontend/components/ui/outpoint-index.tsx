import { NoBreak } from '@/components/ui/no-break';

/**
 * An outpoint's `:index`, after its tx hash: one unit that never breaks inside (":1 / 6"), even in
 * a box that lets the hash itself break anywhere. An inline span, so a name reads "hash:1".
 */
export function OutpointIndex({ index }: { index: number | string }) {
  return <NoBreak data-outpoint-index="">:{index}</NoBreak>;
}
