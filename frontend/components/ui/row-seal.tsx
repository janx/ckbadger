import { HashSeal } from '@/components/ui/hash-seal';
import { cn } from '@/lib/utils';

/** The row seal's height in a list (design 8, the short white-text stone, folded from the page's). */
export const ROW_SEAL_PX = 28;

/**
 * Its drawn width at 28 px: the 7 × 11 stone is 14.48 × 21.12 units, so 19.2 px, written 19
 * (pinned against the drawn `<svg>` in row-seal.test, so a column budget built on it stays true).
 */
export const ROW_SEAL_WIDTH_PX = 19;

/** The space between the seal and the row's key (its number or hash). */
export const ROW_SEAL_GAP_PX = 8;

/** What the seal adds to the minimum width of the list column it leads: the seal and its gap. */
export const ROW_SEAL_FOOTPRINT_PX = ROW_SEAL_WIDTH_PX + ROW_SEAL_GAP_PX;

/**
 * The row seal, first in a list row before the row's key. It is an image named by the hash
 * (HashSeal's label), not a link: the row's own link stays the row's link.
 *
 * `overhang`: the seal stands on the row's one 18 px key line and overhangs it by 5 px above and
 * below (28 = 18 + 2 × 5), inside the cell's 12 px padding, so a one-line table row keeps its
 * height (table-rows T2) with 7 px clear above and below the stone. A row of two lines or more
 * holds the seal without it.
 */
export function RowSeal({ hash, overhang = false }: { hash: string; overhang?: boolean }) {
  return (
    <HashSeal
      hash={hash}
      variant="row"
      size={ROW_SEAL_PX}
      className={cn('mr-2 [&>svg]:block', overhang && '-my-[5px]')}
    />
  );
}
