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
});
