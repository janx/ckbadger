import { droppedGenesCaption, type DroppedGene } from '@/lib/dob-genes';
import { TraitText } from '@/components/asset/trait-text';

/**
 * The one caption under a gel that names the genes without a lane, and why (C9), on the object page
 * and the collection page alike. It is a sentence built from trait values, text anyone can mint, so
 * it breaks anywhere rather than run past the page (the lead's Chrome finding of 2026-11-15: a
 * value of 374 characters with no break overflowed both pages by 1,400–2,300 px).
 */
export function DroppedGenesCaption({
  dropped,
  objects,
}: {
  dropped: readonly DroppedGene[];
  objects: number;
}) {
  if (dropped.length === 0) return null;
  return (
    <p
      data-role="caption"
      // on the measure (readability R4, lane B7): a caller that wants it narrower sets its own
      // box narrower around it, so no class of the caller's can override the measure
      className="text-text-dim text-small max-w-measure [overflow-wrap:anywhere]"
    >
      <TraitText text={droppedGenesCaption(dropped, objects)} />
    </p>
  );
}
