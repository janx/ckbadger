import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { PieChart } from '@/components/ui/pie-chart';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR } from '@/lib/chart-colors';

describe('PieChart', () => {
  it('uses project palette colors by default', () => {
    render(
      <PieChart
        data={[
          { label: 'A', value: 60 },
          { label: 'B', value: 40 },
        ]}
        showLegend={false}
      />
    );

    const slices = document.querySelectorAll('svg path');
    expect(slices[0]).toHaveAttribute('fill', CHART_PRIMARY_COLOR);
    expect(slices[1]).toHaveAttribute('fill', CHART_SECONDARY_COLOR);
  });

  it('keeps explicit slice colors when provided', () => {
    render(
      <PieChart
        data={[
          { label: 'Custom', value: 100, color: '#123456' },
          { label: 'Fallback', value: 1 },
        ]}
        showLegend={false}
      />
    );

    const slices = document.querySelectorAll('svg path');
    expect(slices[0]).toHaveAttribute('fill', '#123456');
    expect(slices[1]).toHaveAttribute('fill', CHART_SECONDARY_COLOR);
  });

  it('renders a bounded chart shell for full-width pies when requested', () => {
    render(
      <PieChart
        data={[
          { label: 'A', value: 60 },
          { label: 'B', value: 40 },
        ]}
        fullWidth
        showLegend={false}
        chartClassName="max-w-[15rem]"
        testIdPrefix="bounded"
      />
    );

    expect(screen.getByTestId('bounded-chart-shell')).toHaveClass('max-w-[15rem]');
  });
});
