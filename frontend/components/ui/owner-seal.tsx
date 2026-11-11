import { Seal } from '@/components/ui/seal';
import { cn } from '@/lib/utils';

/**
 * The owner's stone in px: a script's row stone at 20 px (15 wide), and the space between it and
 * the address it stands before (the round 16 board's 5 px).
 */
export const OWNER_SEAL = { height: 20, width: 15, gap: 5 } as const;

/**
 * The owner's stone (the seals by kind): the row stone of a lock script's hash, an address's own
 * stone, standing before the address's text in a cell row — the owner read from its own stone,
 * not from the cell's small mark. An image named by the hash (its label), not a link: the address
 * beside it keeps its own link. `data-owner-seal` tells it from the row's own seal. `gap={false}`:
 * the stone stands alone in a column of its own (a cell tile's 30 px mark column, C11), whose gap
 * is its gap, so it keeps no 5 px of its own after it.
 */
export function OwnerSeal({
  lockHash,
  gap = true,
  className,
}: {
  lockHash: string;
  gap?: boolean;
  className?: string;
}) {
  return (
    <span
      data-owner-seal=""
      className={cn('inline-flex align-middle', gap && 'mr-[5px]', className)}
    >
      <Seal
        id={{ kind: 'script', hash: lockHash }}
        form="row"
        height={OWNER_SEAL.height}
        className="[&>svg]:block"
      />
    </span>
  );
}
