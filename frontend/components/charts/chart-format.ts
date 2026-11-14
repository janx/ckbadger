/**
 * How the chart pages and the charts index write numbers, units and ranges (UI review 2026-11-14,
 * CH-03, CH-12, CH-14): one place for the chart template's forms, until the site's format helpers
 * (plan phase B3) take them over. Counts are grouped by `formatCount` and exact values by the
 * chart model's `formatChartExact`; this module only joins and spells.
 */

/** A window between two dates: `2026-11-07 → 2026-11-13` (CH-03). */
export function formatDateWindow(from: string, to: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(from) || !/^\d{4}-\d{2}-\d{2}$/.test(to))
    throw new Error(`invalid date window ${from}, ${to}`);
  return `${from} → ${to}`;
}
