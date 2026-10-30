import { beforeEach, describe, expect, it } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';

const points = [
  { label: '0', values: { primary: '4.2' } },
  { label: '6.75', values: { primary: '2.3642' } },
  { label: '20', values: { primary: '1.59' } },
];
const series = [{ key: 'primary', label: 'Nominal APC', color: '#2edba3' }];
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
});
