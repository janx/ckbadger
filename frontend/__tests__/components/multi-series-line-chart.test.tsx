import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import userEvent from '@testing-library/user-event';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';

describe('MultiSeriesLineChart', () => {
  it('toggles series visibility from the legend controls', async () => {
    const user = userEvent.setup();
    const { container } = render(
      <MultiSeriesLineChart
        data={[
          { date: '2026-02-22', values: { holders: '1000', volume: '50' } },
          { date: '2026-02-23', values: { holders: '1100', volume: '60' } },
        ]}
        series={[
          { key: 'holders', label: 'Holder Count', color: '#00c389' },
          { key: 'volume', label: 'Volume', color: '#ffb000' },
        ]}
      />
    );

    expect(screen.getByRole('button', { name: 'Holder Count' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Volume' })).toBeInTheDocument();
    expect(container.querySelectorAll('path[stroke-width="2"]')).toHaveLength(2);

    await user.click(screen.getByRole('button', { name: 'Volume' }));

    expect(container.querySelectorAll('path[stroke-width="2"]')).toHaveLength(1);
  });
});
