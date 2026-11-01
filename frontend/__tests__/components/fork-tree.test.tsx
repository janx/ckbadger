import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { ForkTree } from '@/components/forks/fork-tree';
import { forkTreeModel, type ForkTreeModel } from '@/lib/fork-tree';
import type { ReorgDetail } from '@/lib/api';
import multi from '@/__tests__/fixtures/fork-api-producer/fork-multi-uncle-detail.json';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Every rect and circle of the tree lies inside `width` (nothing to scroll sideways). */
function expectInside(svg: Element, width: number) {
  for (const rect of Array.from(svg.querySelectorAll('rect'))) {
    expect(Number(rect.getAttribute('x'))).toBeGreaterThanOrEqual(0);
    expect(Number(rect.getAttribute('x')) + Number(rect.getAttribute('width'))).toBeLessThanOrEqual(
      width
    );
  }
  for (const circle of Array.from(svg.querySelectorAll('circle'))) {
    const [cx, r] = [Number(circle.getAttribute('cx')), Number(circle.getAttribute('r'))];
    expect(cx - r).toBeGreaterThanOrEqual(0);
    expect(cx + r).toBeLessThanOrEqual(width);
  }
}

describe('ForkTree (Fork detail V67, FF-D4)', () => {
  it('draws main-row tiles, the dashed branch above, one jade arrow per uncle and both tip marks', () => {
    stubChartWidth(1248);
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

  it('fits 358 px: blocks shrink to 72 px, time and miner move under them, nothing scrolls (V4)', () => {
    stubChartWidth(358);
    const model = forkTreeModel(multi as unknown as ReorgDetail);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expect(svg).toHaveAttribute('width', '358');
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 358 /);
    expectInside(svg, 358);
    // the branch line, the dashed lost blocks, the uncle arcs and ① ② are all still drawn
    expect(svg.querySelectorAll('[data-tile][data-dropped] rect')[0]).toHaveAttribute(
      'stroke-dasharray',
      '3 3'
    );
    expect(svg.querySelectorAll('[data-uncle-link]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    // a tile's time sits under its box, not inside it
    const tile = svg.querySelector('[data-tile="11"]:not([data-dropped])')!;
    const box = tile.querySelector('rect')!;
    const time = tile.querySelector('[data-time]')!;
    expect(Number(time.getAttribute('y'))).toBeGreaterThan(
      Number(box.getAttribute('y')) + Number(box.getAttribute('height'))
    );
    expect(Number(box.getAttribute('width'))).toBe(72);
  });

  it('collapses a seven-deep fork’s oldest blocks into "+N earlier" instead of scrolling (V4)', () => {
    stubChartWidth(358);
    const hash = (tag: string) => `0x${tag.padEnd(64, '0')}`;
    const tile = (number: number, tag: string, stub = false) => ({
      number,
      hash: hash(tag),
      timestamp: stub ? null : '2026-11-01T07:47:24.564Z',
      minerLockHash: stub ? null : hash('11'),
      minerAddress: stub ? null : 'ckb1qyqexample93hhsj',
      minerPool: null,
      transactionsCount: stub ? null : 1,
    });
    const deep: ForkTreeModel = {
      main: [
        tile(100, 'f0', true),
        ...[101, 102, 103, 104, 105, 106, 107].map((n) => tile(n, `a${n}`)),
      ],
      rows: [
        {
          label: 'dropped at this switch',
          tiles: [101, 102, 103, 104, 105, 106, 107].map((n) => tile(n, `b${n}`)),
        },
      ],
      uncleLinks: [{ from: hash('b105'), to: 107 }],
      marks: [
        { n: 1, hash: hash('b107') },
        { n: 2, hash: hash('a107') },
      ],
      first: 100,
      last: 107,
    };
    const { container } = render(<ForkTree model={deep} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expectInside(svg, 358);
    const collapsed = svg.querySelector('[data-collapsed]')!;
    expect(collapsed).not.toBeNull();
    expect(screen.getByText(/^\+\d+ earlier$/)).toBeInTheDocument();
    const n = Number(screen.getByText(/^\+\d+ earlier$/).textContent!.match(/\d+/)![0]);
    // the hidden heights are named, and the newest blocks are drawn whole
    expect(collapsed.textContent).toContain(`#100–#${100 + n - 1}`);
    expect(svg.querySelector('[data-tile="107"]:not([data-dropped])')).not.toBeNull();
    expect(svg.querySelector('[data-tile="100"]')).toBeNull();
    expect(svg.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-uncle-link]')).toHaveLength(1);
  });
});
