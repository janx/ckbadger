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
        formatValue={(v) => v}
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
          formatValue={(v) => v}
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
          formatValue={(v) => v}
        />
      )
    ).toThrow(/Bad stack: logarithmic scale requires a positive line series/);
  });
});

describe('shared Chart gaps', () => {
  beforeEach(() => stubChartWidth(480));
  const timed = [
    { label: 'd1', x: 1, values: { value: '5' } },
    { label: 'd2', x: 2, values: { value: '6' } },
    { label: 'd5', x: 5, values: { value: '7' } },
  ];

  it('shades a gap and never bridges a series across it', () => {
    const { container } = render(
      <Chart
        title="Peers"
        subtitle="Daily"
        data={timed}
        series={series}
        unit="peers"
        xDomain={[1, 5]}
        gaps={[{ from: 2, to: 5, label: 'no rounds 3 – 4' }]}
        formatValue={(v) => v}
      />
    );
    expect(container.querySelector('[data-chart-gap="no rounds 3 – 4"] rect')).not.toBeNull();
    const line = container.querySelector('path[fill="none"]')!.getAttribute('d')!;
    expect(line.match(/M/g)).toHaveLength(2);
    expect(container.querySelectorAll('circle')).toHaveLength(1);
  });

  it('rejects a gap that hides an observation or has no time axis', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <Chart
          title="Peers"
          subtitle="Daily"
          data={timed}
          series={series}
          unit="peers"
          xDomain={[1, 5]}
          gaps={[{ from: 1, to: 5, label: 'bad' }]}
          formatValue={(v) => v}
        />
      )
    ).toThrow('gaps must lie on the shared time axis');
    expect(() =>
      render(
        <Chart
          title="Peers"
          subtitle="Daily"
          data={timed}
          series={series}
          unit="peers"
          gaps={[{ from: 2, to: 5, label: 'bad' }]}
          formatValue={(v) => v}
        />
      )
    ).toThrow('gaps must lie on the shared time axis');
    error.mockRestore();
  });
});

describe('shared Chart projection', () => {
  beforeEach(() => stubChartWidth(480));
  const model = ['4.2', '3.1', '2.3', '1.9', '1.6'].map((value, i) => ({
    label: String(i),
    values: { value },
  }));

  it('draws a model solid up to today and dashed ahead (CH-03)', () => {
    const { container } = render(
      <Chart
        title="Nominal APC"
        subtitle="Model"
        data={model}
        series={series}
        unit="%"
        projectedAfter={2}
        formatValue={(v) => v}
      />
    );
    const ahead = container.querySelector('path[data-projected]')!;
    expect(ahead).toHaveAttribute('stroke-dasharray', '4 3');
    expect(ahead.getAttribute('d')!.match(/[ML]/g)).toHaveLength(3);
    expect(container.querySelectorAll('path[fill="none"]:not([data-projected])')).toHaveLength(1);
  });

  it('rejects a projection outside the returned points', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <Chart
          title="Nominal APC"
          subtitle="Model"
          data={model}
          series={series}
          unit="%"
          projectedAfter={5}
          formatValue={(v) => v}
        />
      )
    ).toThrow('projection must start at a returned point');
    error.mockRestore();
  });
});
