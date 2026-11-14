import { expect } from 'vitest';

// U-19 / DY-05 (UI review 2026-11-14): a plot's round values are written in a gutter left of the
// plot, in the dim ink — never inside it, where the data crosses them and a fill buries them.

const isGrid = (line: Element) =>
  line.getAttribute('y1') === line.getAttribute('y2') &&
  (line.getAttribute('stroke') === 'var(--c-base-border-subtle)' ||
    line.classList.contains('stroke-base-border-subtle'));

/** The plot's left edge: where its horizontal grid lines start. */
export function plotLeftOf(svg: Element): number {
  const xs = Array.from(svg.querySelectorAll('line'))
    .filter(isGrid)
    .map((line) => Number(line.getAttribute('x1')));
  if (!xs.length) throw new Error('the plot draws no grid line');
  return Math.min(...xs);
}

/** A plot's value labels wherever drawn: in the plot ([data-role=y-tick]) or in a gutter. */
export function yTickLabels(svg: Element): Element[] {
  const left = plotLeftOf(svg);
  return Array.from(svg.querySelectorAll('text')).filter(
    (t) =>
      t.getAttribute('data-role') === 'y-tick' ||
      (t.getAttribute('text-anchor') === 'end' && Number(t.getAttribute('x')) < left)
  );
}

const dim = (t: Element) =>
  t.getAttribute('fill') === 'var(--c-text-dim)' || t.classList.contains('fill-text-dim');

/**
 * Every value label of the plot ends at least 4 px left of the plot's left edge, anchored at its
 * end, in the dim ink; returns the labels' text.
 */
export function expectTicksInGutter(svg: Element, name: string, minCount = 2): string[] {
  const left = plotLeftOf(svg);
  const labels = yTickLabels(svg);
  expect(labels.length, `${name}: value labels`).toBeGreaterThan(0);
  for (const t of labels) {
    const where = `${name} "${t.textContent}"`;
    expect(t.getAttribute('text-anchor'), `${where} is anchored at its end`).toBe('end');
    expect(
      Number(t.getAttribute('x')),
      `${where} ends left of the plot (${left})`
    ).toBeLessThanOrEqual(left - 4);
    expect(dim(t), `${where} is in the dim ink`).toBe(true);
  }
  expect(labels.length, `${name}: value labels`).toBeGreaterThanOrEqual(minCount);
  return labels.map((t) => t.textContent ?? '');
}
