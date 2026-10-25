import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
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
