import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { BlockContents, ResourceBar, resourceScale } from '@/components/blocks/block-visual';

describe('block evidence visuals', () => {
  it('keeps square area proportional to exact transaction bytes and labels every count', () => {
    const { container } = render(
      <BlockContents transactionSizes={[100, 100, 400]} proposalsCount={3} unclesCount={1} />
    );
    expect(screen.getByRole('img')).toHaveAccessibleName(
      'Cellbase, 2 user transactions, 3 proposals, 1 uncles'
    );
    const squares = container.querySelectorAll('rect');
    expect(Number(squares[1].getAttribute('width')) ** 2).toBe(
      Number(squares[0].getAttribute('width')) ** 2 * 4
    );
  });
  it('labels overhead and user bytes without replacing the block total', () => {
    render(<ResourceBar label="Size, bytes" value={3205} overhead={883} scale={4000} />);
    expect(screen.getByRole('img')).toHaveAccessibleName(
      'Size, bytes: 3,205, 883 overhead, 2,322 user transactions'
    );
    expect(screen.getByText('3,205')).toBeInTheDocument();
    expect(resourceScale([640, 3917])).toBeGreaterThanOrEqual(3917);
  });
  it('starts every inline bar at one left baseline whatever the number width (BA-01)', () => {
    const { container } = render(
      <>
        <ResourceBar label="Cycles" value={7_654_321} scale={8_000_000} />
        <ResourceBar label="Cycles" value={0} scale={8_000_000} />
      </>
    );
    for (const row of Array.from(container.querySelectorAll('[data-aligned-bar]'))) {
      const [track, value] = Array.from(row.children) as HTMLElement[];
      expect(track).toHaveAttribute('role', 'img');
      expect(track.style.width).toBe('120px');
      expect(value.style.width).toBe('9ch');
    }
    expect(container.querySelectorAll('[data-aligned-bar]')).toHaveLength(2);
  });
  it('uses stacked resource bars without shortening an exact cycle count', () => {
    render(<ResourceBar label="Cycles" value={3500000000} scale={4000000000} stacked />);
    const bar = screen.getByRole('img', { name: 'Cycles: 3,500,000,000' });
    expect(bar.parentElement).toHaveClass('flex-col', 'items-end');
    expect(screen.getByText('3,500,000,000')).toHaveClass('order-first');
    expect(bar.lastElementChild).toHaveStyle({ width: '87.5%' });
  });
  it('rejects missing or contradictory evidence', () => {
    expect(() =>
      BlockContents({ transactionSizes: [], proposalsCount: 0, unclesCount: 0 })
    ).toThrow('exact positive size');
    expect(() => ResourceBar({ label: 'bytes', value: 100, overhead: 101, scale: 1000 })).toThrow(
      'overhead'
    );
    expect(() => resourceScale([NaN])).toThrow('exact integers');
  });
});
