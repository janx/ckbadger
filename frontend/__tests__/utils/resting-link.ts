import { expect } from 'vitest';

/**
 * Visual finish V5 (resting chroma): a link inside a table, a feed or a list row rests as ink with
 * a faint aqua underline and has no hover colour of its own — the global `a:hover` turns it jade.
 */
export const RESTING_LINK = [
  'text-text-bright',
  'underline',
  'decoration-aqua/35',
  'underline-offset-[3px]',
  'transition-colors',
];

/** A class that rests a link in the aqua family (#68ccf0 aqua = interactive = info, and -dim). */
export const AQUA_AT_REST = /^(text-aqua|text-interactive|text-info)(-dim)?(\/\d+)?$/;

/** A row link at rest: ink, the faint aqua underline, no aqua class and no hover colour of its own. */
export function expectRestingLink(link: HTMLElement): void {
  expect(link.tagName).toBe('A');
  expect(link).toHaveClass(...RESTING_LINK);
  const classes = Array.from(link.classList);
  expect(classes.filter((c) => AQUA_AT_REST.test(c))).toEqual([]);
  expect(classes.filter((c) => c.startsWith('hover:text-'))).toEqual([]);
}
