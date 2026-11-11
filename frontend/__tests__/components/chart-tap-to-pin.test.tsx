import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, renderHook, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { Chart, type ChartProps } from '@/components/ui/chart';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';

const points = [
  { label: '2026-10-28', values: { rate: '1.5' } },
  { label: '2026-10-29', values: { rate: '2.25' } },
  { label: '2026-10-30', values: { rate: '3' } },
];
const base: ChartProps = {
  title: 'Hash rate',
  subtitle: 'daily',
  data: points,
  series: [{ key: 'rate', label: 'Hash rate', color: 'var(--c-jade)' }],
  unit: 'PH/s',
  formatValue: (v) => v,
  hideCaption: true,
};
// The plot runs from its tick gutter to 470 px of a 480 px box: 470 is the last point, 0 the first.
const LAST = 470;
const FIRST = 0;

describe('Chart tap to pin (R6)', () => {
  beforeEach(() => {
    stubChartWidth(480);
    stubPointerEvents();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const plot = () => screen.getByRole('img', { name: 'Hash rate, PH/s' });
  const readout = () => screen.getByRole('status', { name: 'Hash rate readout' });

  it('pins the nearest point in the line under the plot; a second tap on it clears it', () => {
    render(<Chart {...base} pinnedReadout={{ label: 'Hash rate readout', idle: 'Tap a day.' }} />);
    expect(readout()).toHaveTextContent('Tap a day.');
    expect(readout()).toHaveTextContent('tap or hover');
    // a touch tap: the pointer leaves before the click arrives
    fireEvent.pointerLeave(plot(), { pointerType: 'touch' });
    fireEvent.click(plot(), { clientX: LAST });
    expect(readout()).toHaveTextContent(/^selected 2026-10-30 · 3 PH\/s/);
    // the hover reading still shows over the pin, and the pin returns when it leaves
    fireEvent.pointerMove(plot(), { clientX: FIRST });
    expect(readout()).toHaveTextContent(/^2026-10-28 · 1\.5 PH\/s/);
    fireEvent.pointerLeave(plot());
    expect(readout()).toHaveTextContent(/^selected 2026-10-30 · 3 PH\/s/);
    fireEvent.click(plot(), { clientX: LAST });
    expect(readout()).toHaveTextContent('Tap a day.');
  });

  it('clears the pin on Escape and when the chart is given other points', () => {
    const { rerender } = render(
      <Chart {...base} pinnedReadout={{ label: 'Hash rate readout', idle: 'Tap a day.' }} />
    );
    fireEvent.click(plot(), { clientX: FIRST });
    expect(readout()).toHaveTextContent(/^selected 2026-10-28 · 1\.5 PH\/s/);
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(readout()).toHaveTextContent('Tap a day.');
    fireEvent.click(plot(), { clientX: FIRST });
    // another range: index 0 is now another day, so the pin lets go instead of moving
    rerender(
      <Chart
        {...base}
        data={points.slice(1)}
        pinnedReadout={{ label: 'Hash rate readout', idle: 'Tap a day.' }}
      />
    );
    expect(readout()).toHaveTextContent('Tap a day.');
  });

  it('names each series of a multi-series line, and defers to the key of a keyed chart', () => {
    const two = {
      ...base,
      data: points.map((p) => ({ ...p, values: { rate: p.values.rate, peak: '9' } })),
      series: [
        { key: 'rate', label: 'Rate', color: 'var(--c-jade)' },
        { key: 'peak', label: 'Peak', color: 'var(--c-aqua)' },
      ],
    };
    const { unmount } = render(<Chart {...two} pinnedReadout={{ label: 'Hash rate readout' }} />);
    fireEvent.click(plot(), { clientX: LAST });
    expect(readout()).toHaveTextContent(/^selected 2026-10-30 · Rate 3 PH\/s · Peak 9 PH\/s/);
    unmount();
    render(<Chart {...two} seriesLegend pinnedReadout={{ label: 'Hash rate readout' }} />);
    fireEvent.click(plot(), { clientX: LAST });
    expect(readout()).toHaveTextContent(/^selected 2026-10-30 · values in the key below/);
  });

  it('does not pin without a readout line: a click is only a hover there', () => {
    render(<Chart {...base} />);
    fireEvent.pointerMove(plot(), { clientX: LAST });
    fireEvent.click(plot(), { clientX: LAST });
    fireEvent.pointerLeave(plot());
    expect(screen.queryByRole('status')).toBeNull();
    expect(plot().querySelector('line[stroke="var(--c-text)"]')).toBeNull();
  });

  it('hovers and pins through a caller’s pin state, which reads it', () => {
    const { result } = renderHook(() => usePinnedReadout<number>());
    const { rerender } = render(<Chart {...base} pin={result.current} />);
    fireEvent.click(plot(), { clientX: LAST });
    expect(result.current.pinned).toBe(2);
    rerender(<Chart {...base} pin={result.current} />);
    fireEvent.click(plot(), { clientX: LAST });
    expect(result.current.pinned).toBeNull();
    expect(() =>
      render(
        <Chart {...base} pin={result.current} activeIndex={1} onActiveIndexChange={() => {}} />
      )
    ).toThrow('Hash rate: a chart takes its active point from its pin or its caller, not both');
  });
});
