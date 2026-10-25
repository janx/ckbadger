import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { LineChart } from '@/components/ui/line-chart';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR } from '@/lib/chart-colors';

describe('LineChart', () => {
  it('draws a negative daily flow and keeps its sign in the hover readout', () => {
    const { container } = render(
      <LineChart
        yAxisLabel="Net Flow (CKB/day)"
        data={[
          { date: '2026-02-18', value: '0.00000000' },
          { date: '2026-02-19', value: '-30.00000000' },
        ]}
      />
    );
    const path = screen.getByTestId('line-series-primary');
    expect(path.getAttribute('d')).not.toMatch(/NaN/);
    const endY = Number(path.getAttribute('d')!.match(/L 580 ([\d.]+)/)?.[1]);
    expect(endY).toBeGreaterThanOrEqual(20);
    expect(endY).toBeLessThanOrEqual(200);
    const svg = container.querySelector('svg')!;
    Object.defineProperty(svg, 'getBoundingClientRect', { value: () => ({ left: 0, width: 600 }) });
    fireEvent.mouseMove(svg, { clientX: 580 });
    expect(screen.getByText('-30.00')).toBeInTheDocument();
  });
  it('rejects log scale for nonpositive values without flattening signed points', () => {
    const { container } = render(
      <LineChart
        yAxisLabel="Net Flow"
        data={[
          { date: 'a', value: '0' },
          { date: 'b', value: '-30' },
        ]}
      />
    );
    const logButton = screen.getByRole('button', { name: 'Log Scale' });
    expect(logButton).toBeDisabled();
    fireEvent.click(logButton);
    const path = screen.getByTestId('line-series-primary').getAttribute('d')!;
    expect(path).not.toMatch(/NaN|Infinity/);
    const ys = [...path.matchAll(/[ML] \d+ ([-\d.]+)/g)].map((match) => Number(match[1]));
    expect(ys).toHaveLength(2);
    expect(ys[0]).not.toBe(ys[1]);
    const svg = container.querySelector('svg')!;
    Object.defineProperty(svg, 'getBoundingClientRect', { value: () => ({ left: 0, width: 600 }) });
    fireEvent.mouseMove(svg, { clientX: 580 });
    expect(screen.getByText('-30.00')).toBeInTheDocument();
  });

  it('returns to a linear axis when an active log chart receives a zero value', () => {
    const initial = [
      { date: 'a', value: '10' },
      { date: 'b', value: '100' },
    ];
    const { rerender } = render(<LineChart yAxisLabel="Count" data={initial} />);
    const logButton = screen.getByRole('button', { name: 'Log Scale' });
    fireEvent.click(logButton);
    expect(logButton).toHaveAttribute('aria-pressed', 'true');
    rerender(
      <LineChart
        yAxisLabel="Count"
        data={[
          { date: 'a', value: '0' },
          { date: 'b', value: '100' },
        ]}
      />
    );
    expect(logButton).toBeDisabled();
    expect(logButton).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('line-series-primary').getAttribute('d')).not.toMatch(/NaN|Infinity/);
  });

  it('limits log scale to positive zoom slices and disables it again after reset', () => {
    const data = Array.from({ length: 20 }, (_, i) => ({
      date: String(i),
      value: i < 10 ? String(i + 1) : '0',
    }));
    const { container } = render(<LineChart yAxisLabel="Count" data={data} />);
    const logButton = screen.getByRole('button', { name: 'Log Scale' });
    expect(logButton).toBeDisabled();
    const svg = container.querySelector('svg')!;
    Object.defineProperty(svg, 'getBoundingClientRect', { value: () => ({ left: 0, width: 600 }) });
    fireEvent.mouseDown(svg, { clientX: 60 });
    fireEvent.mouseMove(svg, { clientX: 306 });
    fireEvent.mouseUp(svg, { clientX: 306 });
    expect(logButton).toBeEnabled();
    fireEvent.click(logButton);
    expect(logButton).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Reset Zoom' }));
    expect(logButton).toBeDisabled();
    expect(logButton).toHaveAttribute('aria-pressed', 'false');
  });
  it('keeps positive fractions distinct on the log axis', () => {
    render(
      <LineChart
        yAxisLabel="Rate"
        defaultLogScale
        data={[
          { date: 'a', value: '0.01' },
          { date: 'b', value: '0.1' },
        ]}
      />
    );
    const path = screen.getByTestId('line-series-primary').getAttribute('d')!;
    const ys = [...path.matchAll(/[ML] \d+ ([-\d.]+)/g)].map((match) => Number(match[1]));
    expect(ys).toHaveLength(2);
    expect(ys[0]).not.toBe(ys[1]);
    expect(screen.getByRole('button', { name: 'Log Scale' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
  });
  it('draws positive fractional bars on a finite log axis', () => {
    render(
      <LineChart
        chartType="bar"
        yAxisLabel="Rate"
        defaultLogScale
        data={[
          { date: 'a', value: '0.01' },
          { date: 'b', value: '0.1' },
        ]}
      />
    );
    expect(screen.getByRole('button', { name: 'Log Scale' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
    const bars = screen.getAllByTestId('bar-series-primary');
    expect(bars).toHaveLength(2);
    for (const bar of bars) {
      expect(bar.getAttribute('y')).not.toMatch(/NaN|Infinity/);
      expect(bar.getAttribute('height')).not.toMatch(/NaN|Infinity/);
    }
  });
  it('keeps exact primary and secondary decimals in hover readouts', () => {
    const { container } = render(
      <LineChart
        yAxisLabel="Count"
        y2AxisLabel="Ratio"
        data={[{ date: '2026-10-25', value: '1005', value2: '1.005' }]}
      />
    );
    const svg = container.querySelector('svg')!;
    Object.defineProperty(svg, 'getBoundingClientRect', { value: () => ({ left: 0, width: 600 }) });
    fireEvent.mouseMove(svg, { clientX: 60 });
    expect(screen.getByText('1.01K')).toBeInTheDocument();
    expect(screen.getByText('1.01')).toBeInTheDocument();
  });
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

  it('renders marker links when href is provided', () => {
    render(
      <LineChart
        interactive={false}
        yAxisLabel="Epoch Hours"
        data={[
          { date: '5414', value: '16.2' },
          { date: '12293', value: '16.1' },
        ]}
        markers={[
          { x: '5414', label: 'MIRANA', href: '/blocks/8775638' },
          { x: '12293', label: 'MEEPO' },
        ]}
      />
    );

    expect(screen.getByText('MIRANA').closest('a')).toHaveAttribute('href', '/blocks/8775638');
    expect(screen.getByText('MEEPO').closest('a')).toBeNull();
  });

  it('uses project palette defaults for primary and secondary lines', () => {
    render(
      <LineChart
        interactive={false}
        yAxisLabel="Primary"
        y2AxisLabel="Secondary"
        data={[
          { date: '2026-02-21', value: '10', value2: '3' },
          { date: '2026-02-22', value: '20', value2: '4' },
          { date: '2026-02-23', value: '15', value2: '5' },
        ]}
      />
    );

    expect(screen.getByTestId('line-series-primary')).toHaveAttribute(
      'stroke',
      CHART_PRIMARY_COLOR
    );
    expect(screen.getByTestId('line-series-secondary')).toHaveAttribute(
      'stroke',
      CHART_SECONDARY_COLOR
    );
  });

  it('leaves an undefined secondary ratio as a gap rather than drawing zero', () => {
    render(
      <LineChart
        interactive={false}
        yAxisLabel="Knowledge"
        y2AxisLabel="Utilization"
        data={[
          { date: '2026-10-23', value: '10', value2: '20' },
          { date: '2026-10-24', value: '11', value2: null },
          { date: '2026-10-25', value: '12', value2: '22' },
        ]}
      />
    );
    const path = screen.getByTestId('line-series-secondary').getAttribute('d');
    expect(path).toMatch(/M .* M /);
    expect(path).not.toMatch(/NaN/);
  });
});
