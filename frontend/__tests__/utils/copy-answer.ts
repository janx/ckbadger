import { expect } from 'vitest';

/** A class that clips what overflows its box, at any variant: an overflow other than visible. */
const CLIPS =
  /^(?:[a-z0-9-]+:)*(?:overflow(?:-[xy])?-(?:hidden|auto|scroll|clip)|truncate|line-clamp-\d+)$/;

/**
 * The elements between a copy button's answer and the page that would clip it (lane c2-finish,
 * rulings round 1): the answer stands out of the line's flow under its box, so an ancestor of the
 * copy that clips what overflows it (`overflow-hidden`, a scroller, `truncate`) would cut it off.
 * Each as its tag and the clipping class.
 */
export function clippingAncestors(copyButton: HTMLElement): string[] {
  const out: string[] = [];
  for (let el = copyButton.parentElement?.parentElement ?? null; el; el = el.parentElement) {
    const clip = Array.from(el.classList).find((cls) => CLIPS.test(cls));
    if (clip) out.push(`<${el.tagName.toLowerCase()}> ${clip}`);
  }
  return out;
}

/** The copy button's answer stands out of the line's flow, and nothing above it clips it. */
export function expectAnswerUnclipped(copyButton: HTMLElement) {
  const status = copyButton.parentElement!.querySelector('[role="status"]');
  expect(status, 'the copy button answers in a status').not.toBeNull();
  expect(status).toHaveClass('absolute', 'top-full');
  expect(clippingAncestors(copyButton)).toEqual([]);
}
