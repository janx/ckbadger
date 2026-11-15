import { droppedGenesCaption, type DroppedGene } from '@/lib/dob-genes';
import { cn } from '@/lib/utils';

/**
 * The one caption under a gel that names the genes without a lane, and why (C9), on the object page
 * and the collection page alike. It is a sentence built from trait values, text anyone can mint, so
 * it breaks anywhere rather than run past the page (the lead's Chrome finding of 2026-11-15: a
 * value of 374 characters with no break overflowed both pages by 1,400–2,300 px).
 */
export function DroppedGenesCaption({
  dropped,
  objects,
  className,
}: {
  dropped: readonly DroppedGene[];
  objects: number;
  className?: string;
}) {
  if (dropped.length === 0) return null;
  return (
    <p
      data-role="caption"
      className={cn('text-text-dim text-small [overflow-wrap:anywhere]', className)}
    >
      {droppedGenesCaption(dropped, objects)}
    </p>
  );
}
