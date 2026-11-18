import { describe, expect, it } from 'vitest';
import { formatChartHover, formatChartReadout } from '@/components/charts/chart-model';
import { MINUS } from '@/lib/chart-ticks';

/**
 * A chart's CKB figures in the forms of ruling R5 (readability plan 2026-11-18, lane B2): the
 * detail page's head figure, its readouts and "Last change" take the summary form (whole CKB from
 * 1,000 up, two decimals at most from 1 to 1,000, exact below 1), the index tiles the compact one;
 * both cut, never round. The head carries the exact value in its title (`chart-detail.tsx`).
 * Rates, durations and counts keep their own forms.
 */
describe('a chart’s CKB figures, cut (R5)', () => {
  it('reads a CKB value on the detail page in the summary form', () => {
    expect(formatChartHover('8131306075.44505907', 'CKB')).toBe('8,131,306,075');
    expect(formatChartHover('1000.99999999', 'CKB')).toBe('1,000');
    expect(formatChartHover('999.995', 'CKB')).toBe('999.99');
    expect(formatChartHover('12.5', 'CKB')).toBe('12.5');
    expect(formatChartHover('0.00000001', 'CKB')).toBe('0.00000001');
    expect(formatChartHover('-1234.5', 'CKB')).toBe(`${MINUS}1,234`);
    // a day's net flow, an exact delta with its sign
    expect(formatChartHover('+208801027.17121727', 'CKB/day')).toBe('+208,801,027');
    expect(formatChartHover(`-30`, 'CKB/day')).toBe(`${MINUS}30`);
  });

  it('keeps every other unit as it was: rates at two decimals, other values exact', () => {
    expect(formatChartHover('12.5', '%')).toBe('12.50');
    expect(formatChartHover('16826', 'transactions')).toBe('16,826');
  });

  it('reads a CKB value on a tile in the compact form, cut', () => {
    expect(formatChartReadout('9007199254.740993', 'CKB')).toBe('9.00B');
    expect(formatChartReadout('999999.5', 'CKB')).toBe('999.99k');
    expect(formatChartReadout('99.00000001', 'CKB')).toBe('99.00');
    expect(formatChartReadout('-1234.5', 'CKB')).toBe(`${MINUS}1.23k`);
    expect(formatChartReadout('+208801027.17121727', 'CKB/day')).toBe('+208.80M');
  });
});
