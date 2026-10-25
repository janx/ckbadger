import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
import userEvent from '@testing-library/user-event';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';

describe('MultiSeriesLineChart', () => {
  it('rounds tooltip counts from the exact API integer', () => {
    const { container } = render(
      <MultiSeriesLineChart
        data={[{ date: '2026-02-23', values: { holders: '1005' } }]}
        series={[{ key: 'holders', label: 'Holder Count', color: '#00c389' }]}
      />
    );
    const svg = container.querySelector('svg')!;
    Object.defineProperty(svg, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 600 }),
    });
    fireEvent.mouseMove(svg, { clientX: 60 });
    expect(screen.getByText('1.01K')).toBeInTheDocument();
  });
  it('rejects a missing required series value with date and series context', () => {
    expect(() =>
      render(
        <MultiSeriesLineChart
          data={[{ date: '2026-02-23', values: {} }]}
          series={[{ key: 'holders', label: 'Holder Count', color: '#00c389' }]}
        />
      )
    ).toThrow('2026-02-23: missing holders value');
  });
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
