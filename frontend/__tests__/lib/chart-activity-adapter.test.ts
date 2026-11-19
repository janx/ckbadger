import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '@/lib/api';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';

afterEach(() => vi.restoreAllMocks());

describe('daily activity chart adapter', () => {
  it('uses exact category counts and shannons from one canonical complete-day source', async () => {
    const daily = vi.spyOn(api, 'getDailyActivityStats').mockResolvedValue([
      {
        date: '20261027',
        transferCount: 4294967295,
        daoDepositCount: 4294967295,
        daoWithdrawRequestCount: 4294967295,
        daoWithdrawCompleteCount: 4294967295,
        tokenCount: 4294967295,
        objectCount: 4294967295,
        identityCount: 4294967295,
        scriptCallCount: 4294967295,
        unknownCount: 1,
        coinbaseCount: 1,
        uniqueAddressCount: 4294967295,
        totalCkbMoved: '900719925474099300',
        scriptCounts: [],
        // the API's sum of the eight kinds (8 × 4,294,967,295)
        kindOccurrences: 34359738360,
        // the API's DAO band, the three DAO counts summed by the route (lane D1's third round)
        daoCount: 12884901885,
        // the route's count of the day's user transactions (the P3 follow-up's 2b)
        userTransactionCount: 4294967295,
      },
    ]);
    const volume = chartModel(
      CHART_BY_SLUG.get('activity-volume')!,
      await api.getActivityVolumeChart()
    );
    const types = chartModel(
      CHART_BY_SLUG.get('activity-type-breakdown')!,
      await api.getActivityTypeBreakdownChart()
    );
    const addresses = chartModel(
      CHART_BY_SLUG.get('active-addresses')!,
      await api.getActiveAddressesChart()
    );
    const ckb = chartModel(CHART_BY_SLUG.get('ckb-volume')!, await api.getCkbVolumeChart());
    expect(daily).toHaveBeenCalledTimes(4);
    expect(daily).toHaveBeenCalledWith(0);
    expect(volume.latest?.raw).toBe('34359738360');
    expect(types.points[0].values.dao).toBe('12884901885');
    expect(addresses.latest?.raw).toBe('4294967295');
    expect(ckb.latest?.raw).toBe('9007199254.740993');
    expect(
      [volume, types, addresses, ckb].every((model) => model.points[0].label === '2026-10-27')
    ).toBe(true);
  });

  // lane D1's third round, N7: an older API's day has no kindOccurrences or daoCount; the
  // refusal names the field and says the API is older than the page (it read
  // 'invalid decimal "undefined"')
  it('refuses an older API by the field it does not send', async () => {
    vi.spyOn(api, 'getDailyActivityStats').mockResolvedValue([
      {
        date: '20261027',
        transferCount: 1,
        daoDepositCount: 1,
        daoWithdrawRequestCount: 0,
        daoWithdrawCompleteCount: 0,
        tokenCount: 0,
        objectCount: 0,
        identityCount: 0,
        scriptCallCount: 0,
        unknownCount: 0,
        coinbaseCount: 1,
        uniqueAddressCount: 1,
        totalCkbMoved: '0',
        scriptCounts: [],
      } as never,
    ]);
    await expect(api.getActivityTypeBreakdownChart()).rejects.toThrow(
      // re-pinned on purpose (the third read's La N6): the one reader's sentence (lib/api-fields.ts sentField)
      'daily activities on 20261027: the API sends no kindOccurrences: it is older than this page'
    );
  });
});
