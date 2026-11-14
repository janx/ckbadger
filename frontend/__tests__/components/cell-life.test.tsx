import { describe, expect, it } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { CellLife, CellLifePlaceholder } from '@/components/object/cell-life';

/** Every element under `root` that sets a corner radius in its inline style. */
const inlineRadii = (root: Element) =>
  [root, ...root.querySelectorAll('*')]
    .filter((el) => (el as HTMLElement).style?.borderRadius)
    .map((el) => `${el.tagName.toLowerCase()} ${(el as HTMLElement).style.borderRadius}`);

describe('CellLife thumbnails (U-02, R1: square corners, as the boards)', () => {
  it('draws the placeholder tile square', () => {
    const { container } = render(<CellLifePlaceholder size={56} />);
    expect(container.firstElementChild).toHaveTextContent('?');
    expect(inlineRadii(container.firstElementChild!)).toEqual([]);
  });

  it('draws the living tile, its glow and its canvas square', () => {
    const { container } = render(<CellLife hash={`0x${'ab'.repeat(32)}`} size={56} />);
    const tile = container.firstElementChild as HTMLElement;
    expect(tile.style.width).toBe('56px');
    expect(tile.querySelector('canvas')).not.toBeNull();
    expect(inlineRadii(tile)).toEqual([]);
  });
});
