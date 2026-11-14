/**
 * Square corners (UI review 2026-11-14, U-02, ruling R1): the boards round no frame, card, tile,
 * bar or chip. These read what an SVG drawing rounds, for a test to pin it at none.
 */

/** Every <rect> under `root` (it is never a vacuous check when this is non-empty). */
export function rects(root: ParentNode): SVGRectElement[] {
  return [...root.querySelectorAll('rect')];
}

/**
 * The <rect>s under `root` whose corners are rounded (an `rx` or `ry` other than 0), each written
 * as its tag with its `data-*` attributes and its radius, so a failure names what is round.
 */
export function roundedRects(root: ParentNode): string[] {
  return rects(root)
    .filter((r) =>
      ['rx', 'ry'].some((a) => r.getAttribute(a) !== null && Number(r.getAttribute(a)) !== 0)
    )
    .map((r) => {
      const named = [...r.attributes]
        .filter((a) => a.name.startsWith('data-') || a.name === 'rx' || a.name === 'ry')
        .map((a) => (a.value === '' ? a.name : `${a.name}="${a.value}"`));
      return `<rect ${named.join(' ')}>`;
    });
}
