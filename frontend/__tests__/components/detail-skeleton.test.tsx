import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { DetailSkeleton, DetailSkeletonPage } from '@/components/ui/detail-skeleton';

// Visual finish V8 (plan 2026-11-04): an entity page loads as the shape of what is coming — the
// h1, the lede and the hero as three quiet bars at DetailLayout's spacing — not a "Loading…" line
// or a bare block.
describe('DetailSkeleton', () => {
  it('is one status named "Loading …" holding three pulsing bars: h1, lede, hero', () => {
    render(<DetailSkeleton />);
    const status = screen.getByRole('status', { name: 'Loading …' });
    const bars = [...status.children];
    expect(bars).toHaveLength(3);
    for (const bar of bars) {
      expect(bar).toHaveClass('bg-base-surface', 'animate-pulse', 'rounded');
      expect(bar).toBeEmptyDOMElement();
    }
    const [h1, lede, hero] = bars;
    expect(h1).toHaveClass('h-[30px]', 'w-2/5');
    expect(lede).toHaveClass('h-[13px]', 'w-3/5');
    expect(hero).toHaveClass('h-[72px]', 'w-full');
    // DetailLayout's spacing: the identity line 8 px under the h1, tier one 32 px under the header
    expect(lede).toHaveClass('mt-2');
    expect(hero).toHaveClass('mt-8');
    expect(status).toHaveTextContent('');
  });

  it('brings the page main and its gutters for a page whose layout owns the main', () => {
    render(<DetailSkeletonPage />);
    const main = screen.getByRole('main');
    expect(main).toHaveClass('container', 'mx-auto', 'min-w-0', 'py-8');
    expect(main).toContainElement(screen.getByRole('status', { name: 'Loading …' }));
  });
});
