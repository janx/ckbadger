import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { axisOutliers, chartModel } from '@/components/charts/chart-model';
import { capNote, chartHead } from '@/components/charts/chart-head';
import { CHART_FIXTURES } from './chart-hero-fixtures';

/**
 * The daily activity charts plot the day's activities. While an activity of two kinds counted
 * under both (on 2026-11-18 the filter's All read 6,122 and the kinds summed 6,196), lane D1's
 * second follow-up, item 2, made their words say "kind counts" with the note "An activity of two
 * kinds counts under both.". F-6 makes each activity one kind, so the kinds partition the day's
 * activities: re-pinned on purpose when F-6 merged, the words say activities again and no note
 * speaks of an overlap there is not.
 */

const OVERLAP = /two kinds|kind counts|overlap/i;

describe('the activity charts count activities, which the kinds partition (F-6)', () => {
  it('names the Activities series activities on its plot, its unit, its card and its head', () => {
    const definition = CHART_BY_SLUG.get('activity-volume')!;
    expect(definition.subtitle).toBe('Activities per day');
    expect(definition.plotUnit).toBe('activities a day');
    expect(definition.unit).toBe('activities');
    expect(definition.cardSubtitle).toBe('activities a day');
    const head = chartHead(
      definition,
      chartModel(definition, CHART_FIXTURES.getActivityVolumeChart as never)
    );
    expect(head.eyebrow).toBe('recorded in a day');
    expect(head.figure?.unit).toBe('activities');
    for (const words of [
      definition.subtitle,
      definition.plotUnit,
      definition.cardSubtitle,
      head.eyebrow,
      head.sentence,
    ])
      expect(words).not.toMatch(OVERLAP);
  });

  it('states Activity by type’s whole as the day’s activities, its bands as activities of a kind', () => {
    const definition = CHART_BY_SLUG.get('activity-type-breakdown')!;
    const model = chartModel(definition, CHART_FIXTURES.getActivityTypeBreakdownChart as never);
    expect(definition.subtitle).toBe('Activities per day, by kind');
    expect(definition.plotUnit).toBe('activities a day, by kind');
    const head = chartHead(definition, model);
    // a band is a number of activities of its kind; the kinds partition the day's activities
    expect(head.figure?.unit).toBe('activities');
    expect(head.sentence).toContain("of the day's activities");
    expect(head.sentence).not.toMatch(OVERLAP);
    const outliers = axisOutliers(model.points, model.series, model.kind)!;
    expect(capNote(model, model.points, outliers.indexes, (label) => label, 'day')).toBe(
      `${model.points[50].label} · 1,000,698 activities, above this scale`
    );
  });
});
