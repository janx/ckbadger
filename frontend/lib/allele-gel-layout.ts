/**
 * How an allele gel fits its measured box (ruling R3, `docs/RESPONSIVE_LAYOUT.md` rules 3–4). A
 * lane never goes under 12 px; lanes that do not fit one row wrap to further rows, in DNA order;
 * a lane is given a second row's worth of room when that lets it write its variants in its bands
 * (60 px); and more than 32 genes below md become a ranked gene list, because no phone row can
 * hold them as lanes a reader can tell apart. Pure, so the forms are tested without a browser.
 */

export const ALLELE_GEL = {
  /** The narrowest lane that still holds a band (R3). */
  laneFloor: 12,
  /** A lane this wide writes its variants inside its bands. */
  labelledLane: 60,
  /** More genes than this become the ranked list below `listBelow`. */
  listAbove: 32,
  /** The content width at md (768 − 2 × 16): from here up a many-gene collection is a gel. */
  listBelow: 736,
  /** The left gutter holds each row's genotype label ("this one", an object number). */
  gutter: 62,
  right: 22,
  /** One character of 10 px JetBrains Mono. */
  charWidth: 6.4,
  /** Gene names longer than this are shortened on the gel (the full name is in the lane title). */
  captionChars: 14,
  /** Head of a row whose gene names are written across their lanes. */
  headAcross: 22,
  /** Head of a row whose gene names are turned on end (14 characters at 10 px, and margin). */
  headRotated: 98,
} as const;

export type AlleleGelLayout =
  | { form: 'list' }
  | {
      form: 'gel';
      /** Lane indices, row by row, in DNA order. */
      rows: number[][];
      laneWidth: number;
      gutter: number;
      captions: 'across' | 'rotated';
      head: number;
    };

/** A gene name as drawn on the gel. */
export function geneCaption(gene: string): string {
  const max = ALLELE_GEL.captionChars;
  return gene.length > max ? `${gene.slice(0, max - 1)}…` : gene;
}

export function alleleGelLayout({
  genes,
  width,
  subject,
  overlayLabels = [],
}: {
  genes: readonly string[];
  width: number;
  subject: string;
  overlayLabels?: readonly string[];
}): AlleleGelLayout {
  const g = ALLELE_GEL;
  if (genes.length === 0) throw new Error(`${subject}: allele gel has no genes`);
  if (!Number.isFinite(width) || width <= 0)
    throw new Error(`${subject}: invalid allele gel width ${width}`);
  const n = genes.length;
  if (n > g.listAbove && width < g.listBelow) return { form: 'list' };
  const longest = Math.max(0, ...overlayLabels.map((label) => label.length));
  const gutter = Math.max(g.gutter, Math.ceil(longest * g.charWidth + 4));
  const avail = width - gutter - g.right;
  if (avail < g.laneFloor)
    throw new Error(
      `${subject}: a ${width} px allele gel cannot hold a ${g.laneFloor} px lane (${gutter} px gutter)`
    );
  let perRow: number;
  if (avail / n >= g.labelledLane) perRow = n;
  else if (n > 1 && avail / Math.ceil(n / 2) >= g.labelledLane) perRow = Math.ceil(n / 2);
  else {
    const fit = Math.min(n, Math.floor(avail / g.laneFloor));
    perRow = Math.ceil(n / Math.ceil(n / fit));
  }
  const laneWidth = avail / perRow;
  if (laneWidth < g.laneFloor)
    throw new Error(
      `${subject}: allele gel lane ${laneWidth} px is under the ${g.laneFloor} px floor`
    );
  const rows: number[][] = [];
  for (let i = 0; i < n; i += perRow)
    rows.push(Array.from({ length: Math.min(perRow, n - i) }, (_, k) => i + k));
  const across = genes.every((gene) => geneCaption(gene).length * g.charWidth + 6 <= laneWidth);
  return {
    form: 'gel',
    rows,
    laneWidth,
    gutter,
    captions: across ? 'across' : 'rotated',
    head: across ? g.headAcross : g.headRotated,
  };
}
