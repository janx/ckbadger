import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHARTS } from '@/components/charts/chart-catalogue';
import type { ChartValueAxisInput } from '@/components/ui/chart';
import { MINUS } from '@/lib/chart-ticks';

/**
 * The D1 lane's second follow-up, item 3 (the lead's ruling): an axis tick is a mark at a value the
 * scale chose, and its label is that value — neither a rounded nor a cut form of it, which would put
 * the line at another number than it says. Every chart of the catalogue is drawn on its page and
 * its index card from the hero fixtures (each slug's real domain shapes), in the scale it opens on
 * and, where it offers log, in linear too; each drawn value axis is read back through the chart's
 * own `chartValueAxis`, and every tick's label must parse back to the tick's value exactly.
 */

vi.mock('@/lib/api', async () => {
  const { CHART_FIXTURES: fixtures } = await import('./chart-hero-fixtures');
  const methods = Object.fromEntries(
    Object.keys(fixtures).map((name) => [name, vi.fn(async () => fixtures[name])])
  );
  return {
    api: {
      ...methods,
      getNetworkStats: vi.fn(async () => ({ syncStatus: { chartDataMayBeIncomplete: false } })),
      getHardforks: vi.fn(async () => ({ events: [] })),
      getBlocksByDay: vi.fn(async (date: string) => ({
        date,
        startMs: 0,
        endMs: 86_400_000,
        first: { number: 20_000_000, hash: `0x${'01'.repeat(32)}` },
        last: { number: 20_008_000, hash: `0x${'02'.repeat(32)}` },
        blocksCount: 8001,
        complete: true,
      })),
    },
  };
});
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
const drawn = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', async () => {
  const actual =
    await vi.importActual<typeof import('@/components/ui/chart')>('@/components/ui/chart');
  return {
    ...actual,
    Chart: (props: Record<string, unknown>) => {
      drawn.push(props);
      return <div role="img" aria-label={String(props.title)} />;
    },
  };
});

/** A number's exact decimal digits, sign and exponent resolved: `-1.5e-7` → `-0.00000015`. */
function exactDecimal(text: string): string {
  const match = /^(-)?(\d+)(?:\.(\d+))?(?:e([+-]?\d+))?$/.exec(text);
  if (!match) throw new Error(`not a decimal: ${text}`);
  const [, sign = '', whole, fraction = '', exponent = '0'] = match;
  const digits = `${whole}${fraction}`;
  const point = whole.length + Number(exponent);
  const padded =
    point <= 0
      ? `0.${'0'.repeat(-point)}${digits}`
      : point >= digits.length
        ? `${digits}${'0'.repeat(point - digits.length)}`
        : `${digits.slice(0, point)}.${digits.slice(point)}`;
  const [w, f = ''] = padded.split('.');
  const wholePart = w.replace(/^0+(?=\d)/, '');
  const fractionPart = f.replace(/0+$/, '');
  const out = fractionPart ? `${wholePart}.${fractionPart}` : wholePart;
  return out === '0' ? '0' : `${sign}${out}`;
}

/** What a tick's label says, as an exact decimal: `1.2345k` → `1234.5`, `−6,000` → `-6000`. */
function labelValue(label: string): string {
  const match = /^(−)?([\d,]+(?:\.\d+)?(?:e[+-]?\d+)?)([kMB])?$/.exec(label);
  if (!match) throw new Error(`tick label ${JSON.stringify(label)} is not a bare number`);
  const [, minus, number, suffix] = match;
  const power = suffix ? { k: 3, M: 6, B: 9 }[suffix as 'k' | 'M' | 'B'] : 0;
  const [mantissa, exponent = '0'] = number.replace(/,/g, '').split('e');
  return exactDecimal(`${minus ? '-' : ''}${mantissa}e${Number(exponent) + power}`);
}

/** Every drawn tick whose label is not its value: `slug view: value → label`. */
function offTicks(name: string, props: Record<string, unknown>): string[] {
  const input = props as unknown as ChartValueAxisInput;
  const out: string[] = [];
  const views: [string, ChartValueAxisInput][] = [[String(input.yScale ?? 'linear'), input]];
  // a page that opens on log offers linear too: the reader's other scale
  if (input.yScale === 'log') views.push(['linear', { ...input, yScale: 'linear' }]);
  return views.flatMap(([scale, view]) => {
    const axis = actualChartValueAxis(view);
    if (!axis.ticks) return out;
    return axis.ticks.values.flatMap((value, i) => {
      const tick = scale === 'log' ? 10 ** value : value;
      const said = labelValue(axis.labels[i].replace(MINUS, '−'));
      return said === exactDecimal(String(tick))
        ? []
        : [`${name} ${String(input.title)} (${scale}): ${String(tick)} → ${axis.labels[i]}`];
    });
  });
}

let actualChartValueAxis: typeof import('@/components/ui/chart').chartValueAxis;

beforeEach(async () => {
  drawn.length = 0;
  ({ chartValueAxis: actualChartValueAxis } =
    await vi.importActual<typeof import('@/components/ui/chart')>('@/components/ui/chart'));
});

describe('an axis tick’s label is exactly its value, on every chart page and card', () => {
  it('reads the parser this test reads labels with', () => {
    expect(labelValue('1.2345k')).toBe('1234.5');
    expect(labelValue('−6,000')).toBe('-6000');
    expect(labelValue('2.50')).toBe('2.5');
    expect(labelValue('1e-9')).toBe('0.000000001');
    expect(exactDecimal(String(10 ** 4.5))).toBe('31622.776601683792');
  });

  for (const definition of CHARTS) {
    it(`${definition.slug}: every drawn tick says its value`, async () => {
      render(<ChartDetail slug={definition.slug} />);
      await screen.findByTestId('figure-eyebrow');
      render(<ChartSmallMultiple definition={definition} />);
      await screen.findByRole('group', { name: `${definition.title} current readout` });
      const off = drawn.flatMap((props) => offTicks(definition.slug, props));
      expect(off).toEqual([]);
    });
  }
});
