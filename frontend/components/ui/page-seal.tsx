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

/** A sealed head's seal, and the word its kind line opens on. */
export interface SealedHeadOf extends PageSealOf {
  /**
   * The kind word ("Block", "Transaction", "Cell", "Address"): the kind line's first word, in the
   * dim small voice, beside the stone at every width. It names the page's object, not the stone's
   * kind (an address presses its lock script's stone).
   */
  kind: string;
}

export interface SealedHeadProps extends SealedHeadOf {
  /** The page's status chips, after the kind word on the kind line. */
  status?: ReactNode;
  /** The name: the page's h1. In the plate from md up, under the stone below md, as the id line. */
  children: ReactNode;
  /**
   * The id line: the full hash, out point or address, with its copy control. The plate sets its
   * voice once, the kind line's (text-small in text-text-dim), so what the page gives carries no
   * size or ink of its own (a copy control keeps its own).
   */
  identity: ReactNode;
  /** The rest of the head: under the stone and the plate, from the page's left edge. */
  below?: ReactNode;
  className?: string;
}

/**
 * A page's head with its seal pressed beside the object's name plate, the boards' one corner (the
 * seals by kind, C10). Beside the stone stands only the plate, three short lines on one left edge:
 * the kind line (the kind word in the dim small voice, then the page's status chips), the name
 * (the h1) and the id line (the full hash, out point or address, with its copy control), the id
 * line in the kind line's voice too, set here once (C10 round 2: the four pages' id lines spoke in
 * three sizes and inks, the cell's full out point bright under its h1). Everything else of the
 * head is `below`: under the stone and the plate, from the page's left edge at every width, so the
 * head's prose starts where every section under the head starts.
 *
 * From md up the plate is one column beside the stone, centred on it (`items-center`): its lines 8
 * px apart keep a plate with a one-line name no taller than the stone on every page (kind line 18
 * + name 34.5, the block's 36 + id line 18, or 20 to 26 with a copy button, + 16: 88 to 94.5 px
 * beside stones of 94 to 112). Below md the stone's row carries the kind line alone, centred on
 * the stone; the plate's wrapper is `contents` there, so its name-and-id box is the grid's next
 * item and spans both columns under the stone, at the full width. One DOM in reading order (the
 * stone, the kind line, the name, the id line, below), nothing hidden, the stone at its kind's
 * fixed height (never a fluid scale); the rows 16 px apart below md, 20 from md.
 *
 * `data-sealed-head` names the stone's kind; `data-sealed-plate`, `data-sealed-kind`,
 * `data-sealed-name`, `data-sealed-id` and `data-sealed-below` name the parts.
 */
export function SealedHead({
  id,
  state,
  kind,
  status,
  children,
  identity,
  below,
  className,
}: SealedHeadProps) {
  return (
    <div
      data-sealed-head={id.kind}
      className={cn(
        'grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-center gap-x-4 gap-y-4 md:gap-x-5 md:gap-y-5',
        className
      )}
    >
      <PageSeal id={id} state={state} />
      <div data-sealed-plate="" className="contents md:flex md:min-w-0 md:flex-col md:gap-2">
        <div data-sealed-kind="" className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
          <span className="text-text-dim text-small">{kind}</span>
          {status}
        </div>
        <div data-sealed-name="" className="flex min-w-0 flex-col gap-2 max-md:col-span-2">
          {children}
          <div
            data-sealed-id=""
            className="text-text-dim text-small min-w-0 [overflow-wrap:anywhere]"
          >
            {identity}
          </div>
        </div>
      </div>
      {below !== undefined && (
        <div data-sealed-below="" className="col-span-2 min-w-0">
          {below}
        </div>
      )}
    </div>
  );
}
