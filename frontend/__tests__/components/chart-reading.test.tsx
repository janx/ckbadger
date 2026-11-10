import { beforeEach, describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';

const points = [
  { label: '0', values: { primary: '4.2' } },
  { label: '6.75', values: { primary: '2.3642' } },
  { label: '20', values: { primary: '1.59' } },
];
const series = [{ key: 'primary', label: 'Nominal APC', color: 'var(--c-jade)' }];
const caption = (container: HTMLElement) => container.querySelector('figcaption')!.textContent;

describe('Chart main reading (CH-03)', () => {
  beforeEach(() => stubChartWidth(480));

  it('reads the API-provided reading instead of the last sample', () => {
    const { container } = render(
      <Chart
        title="Nominal APC"
        subtitle="model"
        data={points}
        series={series}
        unit="%"
        reading={{ label: 'today', values: { primary: '2.3642' } }}
        formatValue={(v) => v}
      />
    );
    expect(caption(container)).toContain('2.3642');
    expect(caption(container)).not.toContain('1.59');
  });

  it('shows no main reading when the reading is missing, never the last sample', () => {
    const { container } = render(
      <Chart
        title="Nominal APC"
        subtitle="model"
        data={points}
        series={series}
        unit="%"
        reading={null}
        formatValue={(v) => v}
      />
    );
    expect(caption(container)).not.toContain('1.59');
    expect(caption(container)).not.toContain('%');
  });

  it('titles the chart at body 500 and reads its figure at hero, labels at meta (R1)', () => {
    const { container } = render(
      <Chart
        title="Nominal APC"
        subtitle="model"
        data={points}
        series={series}
        unit="%"
        reading={{ label: 'today', values: { primary: '2.3642' } }}
        formatValue={(v) => v}
      />
    );
    expect(screen.getByText('Nominal APC', { selector: 'figcaption span' })).toHaveClass(
      'text-body',
      'font-medium'
    );
    expect(screen.getByText('model')).toHaveClass('text-meta');
    const value = screen.getByText('2.3642');
    expect(value.parentElement).toHaveClass('text-hero');
    expect(container.querySelector('figcaption')!.className).not.toMatch(/text-\[/);
  });
});
