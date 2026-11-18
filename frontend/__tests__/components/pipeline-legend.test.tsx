import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';
import { tok } from '@/lib/pipeline/encoding';

describe('PipelineLegend', () => {
  it('keeps every stage key reachable when the mobile footer wraps', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    for (const label of ['pending', 'proposed', 'too early', 'committed', 'expired', 'cellbase'])
      expect(legend).toHaveTextContent(label);
    expect(legend).toHaveClass('flex-wrap');
    expect(legend).not.toHaveClass('overflow-hidden');
  });

  it('keys the tx squares and the stage colours with square-cornered swatches (U-02, R1)', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    expect(rects(legend).length).toBeGreaterThanOrEqual(6);
    expect(roundedRects(legend)).toEqual([]);
  });

  // Direction F (Plate.dc.html `.lg`): the unit is said in the legend only, as on /blocks.
  it('opens with the tx square and its scale: "tx, area ∝ bytes", in the muted ink', () => {
    render(<PipelineLegend />);
    const first = screen.getByTestId('pipeline-legend').firstElementChild as HTMLElement;
    expect(first).toHaveTextContent(/^tx, area ∝ bytes$/);
    const sq = first.querySelector('rect') as SVGRectElement;
    expect([sq.getAttribute('width'), sq.getAttribute('height')]).toEqual(['9', '9']);
    expect(sq.style.getPropertyValue('fill')).toBe(tok('ink-dim'));
    expect(first.querySelector('circle')).toBeNull();
    // the swatch and its words never part on a wrap
    expect(first).toHaveClass('whitespace-nowrap');
  });

  // Measured in Chrome (lane F): below about 1,100 px of canvas the legend wraps, and a bar between
  // two groups was left dangling at the end of a line. The wrap and the gap part them there.
  it('parts its three groups of keys with a bar only from xl up, where the legend is one line', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    const items = [...legend.children] as HTMLElement[];
    expect(items.map((el) => el.textContent || '|')).toEqual([
      'tx, area ∝ bytes',
      'proposal',
      'proposal window',
      '|',
      'pending',
      'proposed',
      'too early',
      'committed',
      'expired',
      '|',
      'cellbase',
      'cycles',
      'unknown',
    ]);
    const bars = items.filter((el) => !el.textContent);
    expect(bars).toHaveLength(2);
    for (const bar of bars) {
      expect(bar).toHaveAttribute('aria-hidden');
      expect(bar).toHaveClass('hidden', 'xl:block', 'h-2.5', 'w-px', 'bg-base-border');
    }
  });

  it('draws every square key at 9 px, the canvas’s squares at the legend’s size', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    // the window's swatch is a piece of the strap, not a square
    const squares = rects(legend).filter((r) => !r.closest('[data-swatch="window"]'));
    // tx, five stages, four cycle steps, unknown
    expect(squares).toHaveLength(11);
    for (const r of squares) {
      expect([r.getAttribute('width'), r.getAttribute('height')]).toEqual(['9', '9']);
    }
  });

  // Direction G (Strap.dc.html `.lg .wk`): a third key in the first group, right after "proposal":
  // a 16 × 9 px swatch of the gilt the strap takes inside the window.
  it('keys the proposal window as a 16 × 9 px gilt swatch, right after the proposal', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    const key = legend.children[2] as HTMLElement;
    expect(key).toHaveTextContent(/^proposal window$/);
    expect(key).toHaveClass('whitespace-nowrap');
    const swatch = key.querySelector('[data-swatch="window"]') as SVGRectElement;
    expect(swatch.tagName).toBe('rect');
    expect([swatch.getAttribute('width'), swatch.getAttribute('height')]).toEqual(['16', '9']);
    expect(swatch.style.getPropertyValue('fill')).toBe(tok('gilt'));
    expect(swatch).not.toHaveAttribute('rx');
    expect(key.querySelector('circle')).toBeNull();
  });
});
