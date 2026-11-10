import { useMemo } from 'react';
import { color } from '@/lib/palette';
import { assertSealHash } from '@/lib/talisman/bits';
import { compound } from '@/lib/talisman/compound';
import { RECIPES } from '@/lib/talisman/recipes';
import { cn } from '@/lib/utils';

export type HashSealVariant = 'page' | 'row' | 'brush';

/** Below this many pixels only the row seal is drawn: a taller talisman's strokes merge. */
export const ROW_ONLY_BELOW_PX = 40;

export interface HashSealProps {
  /** A 32-byte hash, lowercase hex with 0x; anything else throws. */
  hash: string;
  /** page: the core talisman (object pages); row: the short white stone (rows); brush: no stone. */
  variant: HashSealVariant;
  /** The seal's longer side in px. */
  size: number;
  /** Degrees. */
  tilt?: number;
  /** The seal's red: frame and strokes, or the white-text stone. A `color()` reference. */
  ink?: string;
  /** The ground the white-text strokes are cut in. A `color()` reference. */
  ground?: string;
  className?: string;
}

/**
 * The hash seal: a talisman drawn from the hash's 256 bits (`lib/talisman`), the same hash always
 * the same seal, the row seal a fold of the page seal (its head, first two compartments, foot).
 *
 * The SVG is the generator's own markup — numbers, fixed element names, the validated hash and
 * the two validated paints — so it is set as inner HTML with nothing to sanitise.
 */
export function HashSeal({
  hash,
  variant,
  size,
  tilt,
  // switches to color('mark') when the palette's seal red lands (rubbing × Song, Phase 1)
  ink = color('rouge'),
  ground = color('base-bg'),
  className,
}: HashSealProps) {
  assertSealHash(hash);
  if (variant !== 'row' && !(size >= ROW_ONLY_BELOW_PX))
    throw new Error(
      `hash seal: the ${variant} seal is drawn at ${ROW_ONLY_BELOW_PX} px or more (asked ${size} px); use the row seal`
    );
  // the markup object itself is memoised: React sets inner HTML again whenever the object changes
  const markup = useMemo(
    () => ({ __html: compound(hash, RECIPES[variant], size, { ink, ground, tilt }).svg }),
    [hash, variant, size, ink, ground, tilt]
  );
  return (
    <span
      data-hash-seal={variant}
      className={cn('inline-block shrink-0 leading-none', className)}
      dangerouslySetInnerHTML={markup}
    />
  );
}
