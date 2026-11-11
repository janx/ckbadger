import type { ReactNode } from 'react';
import { HashSeal } from '@/components/ui/hash-seal';
import { cn } from '@/lib/utils';

/**
 * How large an object page presses its seal (rubbing × Song print, phase 3): from `md` up the
 * wide size, below it (a phone) the narrow one — a fixed smaller size, never a fluid scale.
 */
export const PAGE_SEAL_SIZES = {
  /** A block, a transaction: the chain's own records. */
  record: { wide: 150, narrow: 96 },
  /** An address, a cell. */
  object: { wide: 112, narrow: 80 },
} as const;

export type PageSealScale = keyof typeof PAGE_SEAL_SIZES;

/** Degrees: the stamp is pressed a little off square, as a collector presses one by hand. */
export const PAGE_SEAL_TILT = -4;

/**
 * The narrow size, by CSS on the one drawn seal. The page talisman is drawn in its own units (the
 * viewBox) and the size only sets the `<svg>`'s width and height, so the seal set to 96 px is the
 * seal drawn at 96 px (pinned in page-seal.test); one seal, one image for a screen reader, at
 * every width. `block` drops the inline box's descender space under the image.
 */
const NARROW: Record<PageSealScale, string> = {
  record: 'max-md:[&>svg]:h-[96px] max-md:[&>svg]:w-auto',
  object: 'max-md:[&>svg]:h-[80px] max-md:[&>svg]:w-auto',
};

/** The page seal (design 3, the core talisman) of a 32-byte hash, at its page's size. */
export function PageSeal({ hash, scale }: { hash: string; scale: PageSealScale }) {
  return (
    <HashSeal
      hash={hash}
      variant="page"
      size={PAGE_SEAL_SIZES[scale].wide}
      tilt={PAGE_SEAL_TILT}
      className={cn('[&>svg]:block', NARROW[scale])}
    />
  );
}

/**
 * A page's identity with its seal pressed beside it, the boards' one corner: the seal first, the
 * title and identity lines in the column beside it, top-aligned. The same arrangement at every
 * width; a phone takes the narrow seal, so the identity keeps its lines and the page's first data
 * stays where it was (the seal stands beside lines that were there, not above them).
 *
 * `below` (the Transaction board's comment) sits under the seal and the identity, set in the
 * identity column: a second grid row, so its left edge is the column's at every width and for
 * either seal size, with no number for the seal's drawn width (the talisman is narrower than its
 * size, which is its height).
 */
export function SealedHead({
  hash,
  scale,
  children,
  below,
  className,
}: {
  hash: string;
  scale: PageSealScale;
  children: ReactNode;
  below?: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-sealed-head={scale}
      className={cn(
        'grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-start gap-x-4 md:gap-x-5',
        className
      )}
    >
      <PageSeal hash={hash} scale={scale} />
      <div className="min-w-0">{children}</div>
      {below !== undefined && (
        <div data-sealed-below="" className="col-start-2 min-w-0">
          {below}
        </div>
      )}
    </div>
  );
}
