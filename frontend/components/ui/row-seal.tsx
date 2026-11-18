import { Seal } from '@/components/ui/seal';
import { assertSealId, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import { cn } from '@/lib/utils';

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
}

function measure(height: number, width: number): RowSealMeasure {
  return { height, width, gap: GAP_PX, footprint: width + GAP_PX };
}

/**
 * The seals by kind in a list row (readability ruling R6, Gate F0: a small stone, as in the home
 * page's pipeline, about two thirds of the 26, 24, 30 and 28 px the kinds were, each kind's
 * proportions kept): a block's square stone 16 px (16 wide), a transaction's slab 16 px (32.3
 * wide), a cell's round stone 18 px (18 wide), a script's tall stone 16 px (12 wide). A width is
 * what `drawSeal` draws at the height: a kind's row stone has one proportion for every id (block
 * 8.8 × 8.8 units, transaction about 20.98 × 10.38, cell 2R × 2R, script 7.8 × 10.4), and the
 * drawn width is the height times it, to a tenth of a pixel. No stone is taller than the row line
 * (lib/type-metrics.ts ROW_LINE_PX, 20 px), so none overhangs it.
 */
export const ROW_SEAL: Readonly<Record<SealKind, RowSealMeasure>> = {
  block: measure(16, 16),
  tx: measure(16, 32.3),
  cell: measure(18, 18),
  script: measure(16, 12),
};

/**
 * Which seal a row leads with: the stone of an id, in its `state` (pressed when not given; a
 * consumed cell in the dead ink, a pool transaction unpressed).
 */
export interface RowSealProps {
  id: SealId;
  state?: SealState;
  /**
   * `false`: the stone stands alone in a column of its own (a cell tile's mark column, C11), whose
   * gap is its gap, so it keeps no 8 px of its own after it.
   */
  gap?: boolean;
}

/**
 * The row seal, first in a list row before the row's key. It stands on the row's key line: its box
 * is one row line tall (`h-row`, ROW_LINE_PX) with the stone centred in it, (line − height) / 2
 * clear above and below (2 px for a 16 px stone, 1 px for the cell's 18), so a one-line row keeps
 * its height and in a row of two lines or more the stone stands beside the key, not in the row's
 * middle. It is an image named by the id (its label), not a link: the row's own link stays the
 * row's link.
 */
export function RowSeal({ id, state, gap = true }: RowSealProps) {
  // the kind is checked before it picks the height, so an id that is no seal's is refused by name
  assertSealId(id, 'row');
  return (
    <Seal
      id={id}
      form="row"
      height={ROW_SEAL[id.kind].height}
      state={state}
      className={cn('h-row flex items-center', gap && 'mr-2')}
    />
  );
}
