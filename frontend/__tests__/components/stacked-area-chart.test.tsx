import { fireEvent, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { render } from '../utils/test-utils';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';

describe('StackedAreaChart', () => {
  it('normalizes values when percentage mode is enabled', () => {
    render(
      <StackedAreaChart
        data={[
          {
            date: '2026-02-19',
            values: {
              compensation: '100',
              burnt: '300',
            },
          },
        ]}
        series={[
          { key: 'compensation', label: 'Deposit Compensation', color: '#00c389' },
          { key: 'burnt', label: 'Burnt', color: '#6b7280' },
        ]}
        isPercentage
      />
    );

    const svg = document.querySelector('svg');
    expect(svg).toBeTruthy();

    Object.defineProperty(svg, 'getBoundingClientRect', {
      configurable: true,
      value: () => ({
        x: 0,
        y: 0,
        left: 0,
        top: 0,
        right: 600,
        bottom: 240,
        width: 600,
        height: 240,
        toJSON: () => ({}),
      }),
    });

    fireEvent.mouseMove(svg!, { clientX: 10, clientY: 10 });

    expect(screen.getByText('25.00%')).toBeInTheDocument();
    expect(screen.getByText('75.00%')).toBeInTheDocument();
  });
});
