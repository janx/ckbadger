import { beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';

const points = [
  { label: '2024-01-01', values: { value: '1.00000001' } },
  { label: '2024-02-01', values: { value: '1000000.00000001' } },
];
const series = [{ key: 'value', label: 'Capacity', color: '#2edba3' }];

describe('shared Chart logarithmic geometry', () => {
  beforeEach(() => stubChartWidth(480));

  it('keeps path finite and keyboard readout at exact returned values', () => {
    const { container } = render(
      <Chart
        title="Capacity"
        subtitle="History"
        data={points}
        series={series}
        unit="CKB"
        yScale="log"
      />
    );
    const svg = screen.getByRole('img', { name: 'Capacity, CKB' });
    const path = container.querySelector('path')?.getAttribute('d');
    expect(path).toMatch(/^M/);
    expect(path).not.toMatch(/NaN|Infinity/);
    fireEvent.keyDown(svg, { key: 'ArrowLeft' });
    expect(screen.getByText('1.00000001')).toBeInTheDocument();
    fireEvent.keyDown(svg, { key: 'ArrowRight' });
    expect(screen.getByText('1000000.00000001')).toBeInTheDocument();
  });

  it('rejects a nonpositive or stacked logarithmic series with context', () => {
    expect(() =>
      render(
        <Chart
          title="Bad log"
          subtitle=""
          data={[{ label: 'day', values: { value: '0' } }]}
          series={series}
          unit="CKB"
          yScale="log"
        />
      )
    ).toThrow(/Bad log: logarithmic scale requires a positive line series/);
    expect(() =>
      render(
        <Chart
          title="Bad stack"
          subtitle=""
          data={points}
          series={series}
          unit="CKB"
          kind="stacked"
          yScale="log"
        />
      )
    ).toThrow(/Bad stack: logarithmic scale requires a positive line series/);
  });
});
