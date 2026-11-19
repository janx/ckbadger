import { droppedGenesCaption, type DroppedGene } from '@/lib/dob-genes';
import { TraitText } from '@/components/asset/trait-text';

/**
 * The one caption under a gel that names the genes without a lane, and why (C9), on the object page
 * and the collection page alike. It is a sentence built from trait values, text anyone can mint, so
 * it breaks anywhere rather than run past the page (the lead's Chrome finding of 2026-11-15: a
 * value of 374 characters with no break overflowed both pages by 1,400–2,300 px).
 *
 * It stands on the measure (readability R4, lane B7), and no class of a caller's can override it.
 * On the collection page, under the gel's 440 px column, it is set narrower, 400 px, in a box of
 * its own; the box is drawn with the caption, under the caption's one condition (a gene dropped),
 * so no empty box adds a gap to the column where none is.
 */
export function DroppedGenesCaption({
  dropped,
  objects,
  narrow = false,
}: {
  dropped: readonly DroppedGene[];
  objects: number;
  /** The collection page's gel column: the caption in a 400 px box. */
  narrow?: boolean;
}) {
  if (dropped.length === 0) return null;
  const caption = (
    <p
      data-role="caption"
      className="text-text-dim text-small max-w-measure [overflow-wrap:anywhere]"
    >
      <TraitText text={droppedGenesCaption(dropped, objects)} />
    </p>
  );
  return narrow ? <div className="max-w-[400px]">{caption}</div> : caption;
}
