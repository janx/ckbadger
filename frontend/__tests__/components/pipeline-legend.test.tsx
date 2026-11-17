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

  it('draws every square key at 9 px, the canvas’s squares at the legend’s size', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    const squares = rects(legend);
    // tx, five stages, four cycle steps, unknown
    expect(squares).toHaveLength(11);
    for (const r of squares) {
      expect([r.getAttribute('width'), r.getAttribute('height')]).toEqual(['9', '9']);
    }
  });
});
