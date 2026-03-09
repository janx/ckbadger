import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';

describe('MultiSeriesLineChart', () => {
  it('renders guidance text with improved readable contrast', () => {
    render(
      <MultiSeriesLineChart
        data={[
          { date: '2026-02-22', values: { holders: '1000' } },
          { date: '2026-02-23', values: { holders: '1100' } },
        ]}
        series={[{ key: 'holders', label: 'Holder Count', color: '#00c389' }]}
      />
    );

    const guide = screen.getByText(
      'Drag to select range | Scroll to zoom | Middle-click drag to pan'
    );
    expect(guide).toHaveClass('text-text-muted');
  });
});
