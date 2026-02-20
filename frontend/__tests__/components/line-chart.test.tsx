import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { LineChart } from '@/components/ui/line-chart';

describe('LineChart', () => {
  it('renders bars when chartType is bar', () => {
    render(
      <LineChart
        chartType="bar"
        interactive={false}
        yAxisLabel="Count"
        data={[
          { date: '0-1KB', value: '10' },
          { date: '1-2KB', value: '20' },
          { date: '2-4KB', value: '30' },
        ]}
      />
    );

    expect(screen.getAllByTestId('bar-series-primary')).toHaveLength(3);
    expect(document.querySelectorAll('path')).toHaveLength(0);
  });
});
