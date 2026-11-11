import { HashSeal } from '@/components/ui/hash-seal';
import { Seal } from '@/components/ui/seal';
import { assertSealId, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
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

/** The one line of a list row's key, in px: the line a row seal stands on. */
export const ROW_LINE_PX = 18;

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
    gap: ROW_SEAL_GAP_PX,
    footprint: width + ROW_SEAL_GAP_PX,
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
 * Which seal a row leads with: the stone of an id (`id`, and its `state`: a consumed cell in the
 * dead ink, a pool transaction unpressed), or — until every list has its stone — the talisman of
 * a 32-byte hash. `overhang`: the seal stands on the row's one 18 px key line and overhangs it
 * above and below, inside the cell's padding, so a one-line table row keeps its height; a row of
 * two lines or more holds the seal without it.
 */
export type RowSealProps =
  | { id: SealId; state?: SealState; overhang?: boolean; hash?: never }
  | { hash: string; overhang?: boolean; id?: never; state?: never };

/**
 * The row seal, first in a list row before the row's key. It is an image named by the id (its
 * label), not a link: the row's own link stays the row's link.
 */
export function RowSeal(props: RowSealProps) {
  if (props.id === undefined)
    return <TalismanRowSeal hash={props.hash} overhang={props.overhang ?? false} />;
  const { id, state, overhang = false } = props;
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

/**
 * The talisman's row seal (`overhang`: 5 px above and below, 28 = 18 + 2 × 5, so a one-line table
 * row keeps its height, table-rows T2, with 7 px clear above and below the stone).
 */
function TalismanRowSeal({ hash, overhang }: { hash: string; overhang: boolean }) {
  return (
    <HashSeal
      hash={hash}
      variant="row"
      size={ROW_SEAL_PX}
      className={cn('mr-2 [&>svg]:block', overhang && '-my-[5px]')}
    />
  );
}
