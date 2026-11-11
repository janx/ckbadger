import { Seal } from '@/components/ui/seal';
import { assertSealId, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import { cn } from '@/lib/utils';

/** The one line of a list row's key, in px: the line a row seal stands on. */
export const ROW_LINE_PX = 18;

/** The space between a row stone and the row's key (its number or hash): the seal's `mr-2`. */
const GAP_PX = 8;

/** A stone in a list row, in px. */
export interface RowSealMeasure {
  /** Its height. */
  readonly height: number;
  /** Its drawn width at that height (the same for every id of a kind; pinned in row-seal.test). */
  readonly width: number;
  /** The space between the stone and the row's key. */
  readonly gap: number;
  /** What it adds to the minimum width of the list column it leads: width + gap. */
  readonly footprint: number;
  /** How far it overhangs a one-line row above and below: (height − the 18 px line) / 2. */
  readonly overhang: number;
}

function measure(height: number, width: number): RowSealMeasure {
  return {
    height,
    width,
    gap: GAP_PX,
    footprint: width + GAP_PX,
    overhang: (height - ROW_LINE_PX) / 2,
  };
}

/**
 * The seals by kind in a list row (§0): a block's square stone 26 px (26 wide), a transaction's
 * slab 24 px (48.5 wide), a cell's round stone 30 px (30 wide), a script's tall stone 28 px
 * (21 wide). The widths are what `drawSeal` draws at those heights, for every id.
 */
export const ROW_SEAL: Readonly<Record<SealKind, RowSealMeasure>> = {
  block: measure(26, 26),
  tx: measure(24, 48.5),
  cell: measure(30, 30),
  script: measure(28, 21),
};

/** The overhang of each stone as a class (each a literal, for Tailwind): 4, 3, 6, 5 px. */
const OVERHANG: Record<SealKind, string> = {
  block: '-my-[4px]',
  tx: '-my-[3px]',
  cell: '-my-[6px]',
  script: '-my-[5px]',
};

/**
 * Which seal a row leads with: the stone of an id, in its `state` (pressed when not given; a
 * consumed cell in the dead ink, a pool transaction unpressed). `overhang`: the seal stands on the
 * row's one 18 px key line and overhangs it above and below, inside the cell's padding, so a
 * one-line table row keeps its height; a row of two lines or more holds the seal without it.
 */
export interface RowSealProps {
  id: SealId;
  state?: SealState;
  overhang?: boolean;
}

/**
 * The row seal, first in a list row before the row's key. It is an image named by the id (its
 * label), not a link: the row's own link stays the row's link.
 */
export function RowSeal({ id, state, overhang = false }: RowSealProps) {
  // the kind is checked before it picks the height, so an id that is no seal's is refused by name
  assertSealId(id, 'row');
  return (
    <Seal
      id={id}
      form="row"
      height={ROW_SEAL[id.kind].height}
      state={state}
      className={cn('mr-2 [&>svg]:block', overhang && OVERHANG[id.kind])}
    />
  );
}
