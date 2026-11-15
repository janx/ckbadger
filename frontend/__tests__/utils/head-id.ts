import { expect } from 'vitest';

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
}
