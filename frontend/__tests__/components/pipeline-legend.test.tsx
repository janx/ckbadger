import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';

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
});
