/**
 * The lines a transaction's two sheets draw between and beyond their tiles (seals by kind, C11
 * round 3): every svg path in the cells canvas that is not part of a tile (its seals and class
 * mark) or of a seal (the seam seal's halves) — the continuity links over the seam and the onward
 * bridge, and nothing else.
 */
export function canvasLines(root: ParentNode): SVGPathElement[] {
  return Array.from(root.querySelectorAll<SVGPathElement>('[data-tx-canvas] path')).filter(
    (path) => path.closest('article, [data-seal]') === null
  );
}

/** The overlay over the seam: the svg that is the seam's own child (not the seal's). */
export function seamOverlay(root: ParentNode): SVGSVGElement | null {
  return root.querySelector<SVGSVGElement>('[data-seam] > svg');
}

/** The continuity links: the overlay's paths. */
export function seamLines(root: ParentNode): SVGPathElement[] {
  return Array.from(root.querySelectorAll<SVGPathElement>('[data-seam] > svg path'));
}
