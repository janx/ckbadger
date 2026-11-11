import type { ReactNode } from 'react';
import { Seal } from '@/components/ui/seal';
import { assertSealId, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import { cn } from '@/lib/utils';

/**
 * How tall an object page presses the stone of its kind (the seals by kind, §0): from `md` up the
 * wide height, below it (a phone) the narrow one — a fixed smaller height, never a fluid scale. The
 * height is set and the width follows: the block and the cell are as wide as tall, the
 * transaction's long stone about 1.63 times (153.5 × 94, 104.5 × 64), the script's tall one about
 * 0.71 (73.5 × 104, 56.5 × 80).
 */
export const PAGE_SEAL_HEIGHTS = {
  block: { wide: 104, narrow: 80 },
  tx: { wide: 94, narrow: 64 },
  cell: { wide: 112, narrow: 88 },
  script: { wide: 104, narrow: 80 },
} as const satisfies Record<SealKind, { wide: number; narrow: number }>;

/** Degrees: the stamp is pressed a little off square, as a collector presses one by hand. */
export const PAGE_SEAL_TILT = -4;

/**
 * The narrow height of each stone, by CSS on the one drawn seal, the width following (`w-auto`:
 * the svg's own ratio, so the transaction's long stone narrows with its height). A page stone is
 * drawn in its own units and its height writes nothing but the svg's width and height (pinned in
 * page-seal.test), so the stone set to 64 px is the stone drawn at 64 px; one seal, one image for
 * a screen reader, at every width. `block` drops the inline box's descender space under the image.
 */
const NARROW_HEIGHT: Record<SealKind, string> = {
  block: 'max-md:[&>svg]:h-[80px] max-md:[&>svg]:w-auto',
  tx: 'max-md:[&>svg]:h-[64px] max-md:[&>svg]:w-auto',
  cell: 'max-md:[&>svg]:h-[88px] max-md:[&>svg]:w-auto',
  script: 'max-md:[&>svg]:h-[80px] max-md:[&>svg]:w-auto',
};

/**
 * Which seal a page presses: the stone of an id, in its `state` (pressed when not given; a
 * consumed cell in the dead ink, a transaction not yet committed unpressed).
 */
export interface PageSealOf {
  id: SealId;
  state?: SealState;
}

/** The page seal of an id: its stone at its kind's height, tilted. */
export function PageSeal({ id, state }: PageSealOf) {
  // the kind is checked before it picks the height, so an id that is no seal's is refused by name
  assertSealId(id, 'page');
  return (
    <Seal
      id={id}
      form="page"
      height={PAGE_SEAL_HEIGHTS[id.kind].wide}
      state={state}
      tilt={PAGE_SEAL_TILT}
      className={cn('[&>svg]:block', NARROW_HEIGHT[id.kind])}
    />
  );
}

export interface SealedHeadProps extends PageSealOf {
  children: ReactNode;
  below?: ReactNode;
  className?: string;
}

/**
 * A page's identity with its seal pressed beside it, the boards' one corner: the seal first, the
 * title and identity lines in the column beside it, top-aligned. The same arrangement at every
 * width; a phone takes the narrow seal, so the identity keeps its lines and the page's first data
 * stays where it was (the seal stands beside lines that were there, not above them).
 *
 * `below` (the Transaction board's comment) sits under the seal and the identity, set in the
 * identity column: a second grid row, so its left edge is the column's at every width and for
 * every stone, with no number for the seal's drawn width (a stone's width follows its height and
 * its kind).
 *
 * `data-sealed-head` names the stone's kind.
 */
export function SealedHead({ id, state, children, below, className }: SealedHeadProps) {
  return (
    <div
      data-sealed-head={id.kind}
      className={cn(
        'grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-start gap-x-4 md:gap-x-5',
        className
      )}
    >
      <PageSeal id={id} state={state} />
      <div className="min-w-0">{children}</div>
      {below !== undefined && (
        <div data-sealed-below="" className="col-start-2 min-w-0">
          {below}
        </div>
      )}
    </div>
  );
}
