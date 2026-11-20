import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { color } from '@/lib/palette';

// UI review 2026-11-14, CH-02: "Activity by type" drew its six types as six steps of one celadon
// ramp, the largest (script calls) the faintest, so the plot was nearly invisible in both themes.
// The types are the classes the rest of the site colours: transfers the data ink, DAO gold,
// tokens azurite, objects lotus violet, identities rose, script calls the moss "script call" ink.

// F-6 (2026-11-18): one kind per transaction, so Protocols is a band of its own.
const KEYS = ['transfer', 'dao', 'token', 'object', 'identity', 'scriptCall', 'protocol'] as const;
const payload = {
  title: 'Activity Type Breakdown',
  series: KEYS.map((key) => ({ key, label: key, color: '#000' })),
  data: [
    {
      date: '2026-11-13',
      values: {
        transfer: '1323',
        dao: '27',
        token: '2',
        object: '2',
        identity: '22',
        scriptCall: '6317',
        protocol: '6',
        // the day's activities, the route's: the seven kinds partition it (F-6)
        activityCount: '7699',
      },
    },
  ],
};
const CLASS_INK: Record<(typeof KEYS)[number], string> = {
  transfer: DATA_COLOR,
  dao: CLASS_COLORS.dao,
  token: CLASS_COLORS.tokens,
  object: CLASS_COLORS.objects,
  identity: CLASS_COLORS.identities,
  scriptCall: color('amber'),
  protocol: color('ink-muted'),
};

describe('Activity by type in the class colours (CH-02)', () => {
  const model = chartModel(CHART_BY_SLUG.get('activity-type-breakdown')!, payload);

  it('draws each type in its class ink at full strength on the page', () => {
    expect(model.series.map((series) => [series.key, series.color])).toEqual(
      KEYS.map((key) => [key, CLASS_INK[key]])
    );
  });

  it('gives the index card the same colours, type by type', () => {
    const card = model.card!;
    expect(card.series.map((series) => [series.key, series.color])).toEqual(
      expect.arrayContaining(KEYS.map((key) => [key, CLASS_INK[key]]))
    );
    expect(card.series).toHaveLength(KEYS.length);
    for (const entry of card.legend)
      expect(entry.color).toBe(CLASS_INK[entry.key as (typeof KEYS)[number]]);
  });
});
