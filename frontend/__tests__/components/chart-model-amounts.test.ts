import { describe, expect, it } from 'vitest';
import {
  formatChartHead,
  formatChartHover,
  formatChartReadout,
} from '@/components/charts/chart-model';
import { MINUS } from '@/lib/chart-ticks';

/**
 * A chart's CKB figures in the forms of ruling R5 (readability plan 2026-11-18, lane B2): the
 * detail page's head figure takes the summary form (whole CKB from 1,000 up, two decimals at most
 * from 1 to 1,000, exact below 1), cut, never rounded, its exact value in its title
 * (`chart-detail.tsx`); a readout (hover, in-plot, legend, "Last change") is where a point's value
 * is read and states it exactly; the index tiles take the compact form, cut. Rates, durations and
 * counts keep their own forms.
 */
describe('a chart’s CKB figures (R5)', () => {
  it('heads the detail page with a CKB value in the summary form', () => {
    expect(formatChartHead('8131306075.44505907', 'CKB')).toBe('8,131,306,075');
    expect(formatChartHead('1000.99999999', 'CKB')).toBe('1,000');
    expect(formatChartHead('999.995', 'CKB')).toBe('999.99');
    expect(formatChartHead('12.5', 'CKB')).toBe('12.5');
    expect(formatChartHead('0.00000001', 'CKB')).toBe('0.00000001');
    expect(formatChartHead('-1234.5', 'CKB')).toBe(`${MINUS}1,234`);
    // a day's net flow, an exact delta with its sign
    expect(formatChartHead('+208801027.17121727', 'CKB/day')).toBe('+208,801,027');
    expect(formatChartHead(`-30`, 'CKB/day')).toBe(`${MINUS}30`);
  });

  // Re-pinned on purpose (review of main..fix/readability, logic finding 1): these readouts were
  // the summary form ('8,131,306,075', '1,000', '999.99', '−1,234', '+208,801,027'), and nothing
  // on the page carried the cut decimals of any point but the latest.
  it('reads a CKB value exactly in a readout: trailing zeros dropped, never padded, never cut', () => {
    expect(formatChartHover('8131306075.44505907', 'CKB')).toBe('8,131,306,075.44505907');
    expect(formatChartHover('1000.99999999', 'CKB')).toBe('1,000.99999999');
    expect(formatChartHover('999.995', 'CKB')).toBe('999.995');
    expect(formatChartHover('12.50000000', 'CKB')).toBe('12.5');
    expect(formatChartHover('1000.00000000', 'CKB')).toBe('1,000');
    expect(formatChartHover('0.00000001', 'CKB')).toBe('0.00000001');
    expect(formatChartHover('-1234.5', 'CKB')).toBe(`${MINUS}1,234.5`);
    expect(formatChartHover('+208801027.17121727', 'CKB/day')).toBe('+208,801,027.17121727');
    expect(formatChartHover(`-30`, 'CKB/day')).toBe(`${MINUS}30`);
  });

  it('keeps every other unit as it was: rates at two decimals, other values exact', () => {
    expect(formatChartHover('12.5', '%')).toBe('12.50');
    expect(formatChartHover('16826', 'transactions')).toBe('16,826');
    expect(formatChartHead('12.5', '%')).toBe('12.50');
    expect(formatChartHead('16826', 'transactions')).toBe('16,826');
  });

  it('reads a CKB value on a tile in the compact form, cut', () => {
    expect(formatChartReadout('9007199254.740993', 'CKB')).toBe('9.00B');
    expect(formatChartReadout('999999.5', 'CKB')).toBe('999.99k');
    expect(formatChartReadout('99.00000001', 'CKB')).toBe('99.00');
    expect(formatChartReadout('-1234.5', 'CKB')).toBe(`${MINUS}1.23k`);
    expect(formatChartReadout('+208801027.17121727', 'CKB/day')).toBe('+208.80M');
  });
});
