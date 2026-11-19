import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '@/lib/api';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';

afterEach(() => vi.restoreAllMocks());

describe('daily activity chart adapter', () => {
  it('uses exact kind counts, the API total and shannons from one canonical complete-day source', async () => {
    const max = 4294967295;
    const daily = vi.spyOn(api, 'getDailyActivityStats').mockResolvedValue([
      {
        date: '20261027',
        // Seven u32 kinds at their maximum; their total is the API's one number, never summed here.
        activityCount: 7 * max,
        kindCounts: {
          dao: max,
          token: max,
          object: max,
          identity: max,
          protocol: max,
          script: max,
          ckb: max,
        },
        coinbaseCount: 1,
        uniqueAddressCount: max,
        totalCkbMoved: '900719925474099300',
        scriptCounts: [],
        protocolActionCounts: { 'dao:deposit': max },
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
    expect(volume.latest?.raw).toBe('30064771065');
    expect(types.points[0].values.dao).toBe('4294967295');
    // F-6: the protocol kind is a band of its own, and the stack is a partition of the total.
    expect(types.points[0].values.protocol).toBe('4294967295');
    expect(
      Object.values(types.points[0].values).reduce((sum, v) => sum + BigInt(v as string), BigInt(0))
    ).toBe(BigInt(30064771065));
    expect(addresses.latest?.raw).toBe('4294967295');
    expect(ckb.latest?.raw).toBe('9007199254.740993');
    expect(
      [volume, types, addresses, ckb].every((model) => model.points[0].label === '2026-10-27')
    ).toBe(true);
  });

  // lane D1's third round, N7: an older API's day has no kindCounts; the refusal names the field
  // and says the API is older than the page (it read 'invalid decimal "undefined"'). Re-pinned on
  // purpose (F-6 merged): the page reads the route's kindCounts, so that is the field it names.
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
      'daily activities on 20261027: the API sends no kindCounts: it is older than this page'
    );
  });
});
