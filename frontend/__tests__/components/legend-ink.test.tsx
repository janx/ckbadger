import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart, type ChartProps } from '@/components/ui/chart';
import { CellBar } from '@/components/ui/cell-bar';
import { CELL_COLORS, CLASS_COLORS } from '@/lib/design-tokens';

// UI review 2026-11-14 (U-08): a legend's label is text, so it is a text ink; the series colour is
// on the swatch only. Coloured labels read as low as 1.4:1 ("Other scripts" in the "other" slate).

const two: ChartProps = {
  title: 'Common knowledge',
  subtitle: 'daily',
  data: [
    { label: '2026-10-29', values: { dao: '2', other: '1' } },
    { label: '2026-10-30', values: { dao: '3', other: '1.5' } },
  ],
  series: [
    { key: 'dao', label: 'Nervos DAO', color: CLASS_COLORS.dao },
    { key: 'other', label: 'Other scripts', color: CLASS_COLORS.other },
  ],
  unit: 'CKB',
  formatValue: (v) => v,
};

/** The inline colour of an element and of every element in it. */
function inlineColours(element: Element): string[] {
  return [element, ...element.querySelectorAll('*')]
    .map((e) => (e as HTMLElement).style.color)
    .filter(Boolean);
}

describe('a legend label is text; the colour is on its swatch (U-08)', () => {
  beforeEach(() => stubChartWidth(480));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('names the series of a chart’s head readout in the text ink, each after its swatch', () => {
    render(<Chart {...two} />);
    for (const [i, name] of ['Nervos DAO', 'Other scripts'].entries()) {
      const label = screen.getByText(name);
      expect(label, name).toHaveClass('text-text');
      expect(inlineColours(label), name).toEqual([]);
      const swatch = label.querySelector('[data-swatch]') as HTMLElement;
      expect(swatch, name).not.toBeNull();
      expect(swatch.style.backgroundColor, name).toBe(two.series[i].color);
      expect(swatch).toHaveAttribute('aria-hidden', 'true');
    }
  });

  it('keeps the key under a chart in the text ink with its swatch', () => {
    render(<Chart {...two} seriesLegend />);
    const key = screen.getByRole('list', { name: 'Common knowledge legend' });
    for (const [i, name] of ['Nervos DAO', 'Other scripts'].entries()) {
      const label = within(key).getByText(name);
      expect(label, name).toHaveClass('text-text');
      expect(inlineColours(label), name).toEqual([]);
      const swatch = label.parentElement!.querySelector('[data-swatch]') as HTMLElement;
      expect(swatch.style.backgroundColor, name).toBe(two.series[i].color);
    }
  });

  it('names the parts of a cell’s bar in the text ink, each after its swatch', () => {
    const { container } = render(
      <CellBar subject="0xabc:0" anatomy={{ lock: '53', type: '33', data: '16' }} />
    );
    const legend = container.querySelector('[data-role="cell-bar-legend"]') as HTMLElement;
    expect(legend).not.toBeNull();
    expect(inlineColours(legend)).toEqual([]);
    for (const part of ['capacity', 'lock', 'type', 'data'] as const) {
      const entry = legend.querySelector(`[data-part-legend="${part}"]`) as HTMLElement;
      expect(entry, part).not.toBeNull();
      const swatch = entry.querySelector('[data-swatch]') as HTMLElement;
      expect(swatch.style.backgroundColor, part).toBe(CELL_COLORS[part]);
    }
  });
});
