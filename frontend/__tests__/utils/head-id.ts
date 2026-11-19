import { expect } from 'vitest';
import { expectAnswerUnclipped } from '@/__tests__/utils/copy-answer';
import { widthRuleAt } from '@/__tests__/utils/tailwind-box';

/**
 * A4 Chrome F3: a full id in a page's head is the head's HeadId — one line where the measured line
 * holds it, else equal unbreakable lines — never the own-box scroller (HexCode's full value), in
 * the head's voice (no size or ink of its own). The page's header holds no sideways scroller.
 */
export function expectHeadId(value: string) {
  const header = document.querySelector('header') as HTMLElement;
  expect(header).not.toBeNull();
  const id = header.querySelector('[data-head-id]') as HTMLElement;
  expect(id, 'the head prints its id through HeadId').not.toBeNull();
  expect(id.textContent).toBe(value);
  expect(id).toHaveAttribute('title', value);
  expect(id.className).not.toMatch(/(^|\s)text-/);
  expect(header.querySelector('.overflow-x-auto')).toBeNull();
  expect(header.querySelector('[data-full-value]')).toBeNull();
  // lane c2-finish (item 4, and rulings round 1): the copy button after the id, its answer out
  // of the line's flow with nothing above it to clip it
  const copy = id.parentElement!.querySelector<HTMLElement>(':scope > span > button');
  expect(copy, 'the head’s copy button stands after its id').not.toBeNull();
  expectAnswerUnclipped(copy!);
  // rulings round 2: the id takes its content's width, so its copy stands 8 px after it
  for (const w of [1280, 390]) expect(widthRuleAt(id, w), `the id at ${w}`).toBe('auto');
}
