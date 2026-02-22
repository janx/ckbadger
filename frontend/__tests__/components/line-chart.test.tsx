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

  it('renders marker lines for matching x values', () => {
    render(
      <LineChart
        interactive={false}
        yAxisLabel="Epoch Hours"
        data={[
          { date: '5414', value: '16.2' },
          { date: '5415', value: '16.3' },
          { date: '12293', value: '16.1' },
        ]}
        markers={[
          { x: '5414', label: 'MIRANA' },
          { x: '12293', label: 'MEEPO' },
          { x: '99999', label: 'NOT_FOUND' },
        ]}
      />
    );

    expect(screen.getAllByTestId('line-chart-marker-line')).toHaveLength(2);
    expect(screen.getByText('MIRANA')).toBeInTheDocument();
    expect(screen.getByText('MEEPO')).toBeInTheDocument();
    expect(screen.queryByText('NOT_FOUND')).not.toBeInTheDocument();
  });
});
