import { describe, expect, it } from 'vitest';
import { render } from '../utils/test-utils';
import { ForkTree } from '@/components/forks/fork-tree';
import { forkTreeModel } from '@/lib/fork-tree';
import type { ReorgDetail } from '@/lib/api';
import multi from '@/__tests__/fixtures/fork-api-producer/fork-multi-uncle-detail.json';

describe('ForkTree (Fork detail V67, FF-D4)', () => {
  it('draws main-row tiles, the dashed branch above, one jade arrow per uncle and both tip marks', () => {
    const model = forkTreeModel(multi as unknown as ReorgDetail);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    const tiles = Array.from(svg.querySelectorAll('[data-tile]'));
    expect(
      tiles.filter((t) => !t.hasAttribute('data-dropped')).map((t) => t.getAttribute('data-tile'))
    ).toEqual(['10', '11', '12', '13']);
    const dropped = svg.querySelectorAll('[data-tile][data-dropped] rect');
    expect(dropped).toHaveLength(2);
    expect(dropped[0]).toHaveAttribute('stroke-dasharray', '3 3');
    const links = svg.querySelectorAll('[data-uncle-link]');
    expect(links).toHaveLength(2);
    expect(links[0]).toHaveAttribute('stroke', '#2edba3');
    expect(Array.from(svg.querySelectorAll('[data-tip-mark]')).map((m) => m.textContent)).toEqual([
      '1',
      '2',
    ]);
    // dropped tiles sit above the main row
    const y = (sel: string) => Number(svg.querySelector(sel)!.getAttribute('y'));
    expect(y('[data-tile][data-dropped] rect')).toBeLessThan(
      y('[data-tile="11"]:not([data-dropped]) rect')
    );
    // drawn at its natural size: 1:1 text at 1440, a sideways scroll at 390 (visual pass)
    const viewBoxWidth = svg.getAttribute('viewBox')!.split(' ')[2];
    expect(svg.getAttribute('width')).toBe(viewBoxWidth);
    expect(svg).not.toHaveAttribute('width', '100%');
  });
});
