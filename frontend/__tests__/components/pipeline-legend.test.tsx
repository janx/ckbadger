import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { PipelineLegend } from '@/components/pipeline/pipeline-legend';

describe('PipelineLegend', () => {
  it('keeps every stage key reachable when the mobile footer wraps', () => {
    render(<PipelineLegend />);
    const legend = screen.getByTestId('pipeline-legend');
    for (const label of ['pending', 'proposed', 'too early', 'committed', 'expired', 'cellbase'])
      expect(legend).toHaveTextContent(label);
    expect(legend).toHaveClass('flex-wrap');
    expect(legend).not.toHaveClass('overflow-hidden');
  });
});
