import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { axisOutliers, chartModel } from '@/components/charts/chart-model';
import { capNote, chartHead } from '@/components/charts/chart-head';
import { CHART_FIXTURES } from './chart-hero-fixtures';

/**
 * Lane D1, second follow-up, item 2: the daily activity charts plot the day's kind counts, in which
 * an activity of two kinds counts under both (on 2026-11-18 the filter's All read 6,122 and the
 * kinds summed 6,196), until `fix/activity-kinds` makes the kinds partition the day's activities.
 * Until then the words say what is plotted: kind counts, and once that an activity of two kinds
 * counts under both; no word claims a number of activities.
 */

const NOTE = 'An activity of two kinds counts under both.';

describe('the activity charts say they count kinds (lane D1, second follow-up, item 2)', () => {
  it('names the Activities series kind counts on its plot, its unit, its card and its head', () => {
    const definition = CHART_BY_SLUG.get('activity-volume')!;
    expect(definition.plotUnit).toBe('kind counts a day');
    expect(definition.unit).toBe('kind counts');
    expect(definition.cardSubtitle).toBe('kind counts a day');
    const head = chartHead(
      definition,
      chartModel(definition, CHART_FIXTURES.getActivityVolumeChart as never)
    );
    expect(head.eyebrow).toBe('counted in a day');
    expect(head.figure?.unit).toBe('kind counts');
    expect(head.sentence.endsWith(` ${NOTE}`)).toBe(true);
    // no word of the page claims a number of activities
    for (const words of [
      definition.subtitle,
      definition.plotUnit,
      definition.cardSubtitle,
      head.eyebrow,
      head.sentence,
    ])
      expect(words).not.toMatch(/activities/i);
  });

  it('states Activity by type’s whole as kind counts, its bands as activities of a kind', () => {
    const definition = CHART_BY_SLUG.get('activity-type-breakdown')!;
    const model = chartModel(definition, CHART_FIXTURES.getActivityTypeBreakdownChart as never);
    expect(definition.plotUnit).toBe('kind counts a day, by kind');
    const head = chartHead(definition, model);
    // a band is a number of activities of its kind; the stack sums the kinds
    expect(head.figure?.unit).toBe('activities');
    expect(head.sentence).toContain("of the day's kind counts");
    expect(head.sentence.endsWith(` ${NOTE}`)).toBe(true);
    const outliers = axisOutliers(model.points, model.series, model.kind)!;
    expect(capNote(model, model.points, outliers.indexes, (label) => label, 'day')).toBe(
      `${model.points[50].label} · 1,000,698 kind counts, above this scale`
    );
  });
});
