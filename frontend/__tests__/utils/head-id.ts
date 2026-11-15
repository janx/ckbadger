import { expect } from 'vitest';

/**
 * A4 Chrome F3: a full id in a page's head is the head's HeadId — one line where the measured line
 * holds it, else equal unbreakable lines — never the own-box scroller (HexCode's full value), at
 * the step the head sets. The page's header holds no sideways scroller.
 */
export function expectHeadId(value: string, step: 'meta' | 'small') {
  const header = document.querySelector('header') as HTMLElement;
  expect(header).not.toBeNull();
  const id = header.querySelector('[data-head-id]') as HTMLElement;
  expect(id, 'the head prints its id through HeadId').not.toBeNull();
  expect(id.textContent).toBe(value);
  expect(id).toHaveAttribute('title', value);
  expect(id).toHaveClass(step === 'meta' ? 'text-meta' : 'text-small');
  expect(header.querySelector('.overflow-x-auto')).toBeNull();
  expect(header.querySelector('[data-full-value]')).toBeNull();
}
