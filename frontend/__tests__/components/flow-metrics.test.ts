import { describe, expect, it } from 'vitest';
import {
  buildMetricDomain,
  mapTxToScatterPoint,
  type FlowMetricTx,
} from '@/components/chain-wave/flow-metrics';

describe('flow-metrics', () => {
  it('builds domain from transaction values', () => {
    const items: FlowMetricTx[] = [
      { size: 300, feeRate: 10, cycles: 20_000 },
      { size: 4_000, feeRate: 120, cycles: 2_000_000 },
      { size: 1_200, feeRate: 40, cycles: 400_000 },
    ];

    const domain = buildMetricDomain(items);

    expect(domain.sizeMin).toBe(300);
    expect(domain.sizeMax).toBe(4_000);
    expect(domain.feeRateMin).toBe(10);
    expect(domain.feeRateMax).toBe(120);
    expect(domain.cyclesMin).toBe(20_000);
    expect(domain.cyclesMax).toBe(2_000_000);
  });

  it('falls back to defaults when optional metrics are missing', () => {
    const items: FlowMetricTx[] = [{ size: 500 }, { size: 900 }];
    const domain = buildMetricDomain(items);

    expect(domain.feeRateMin).toBeGreaterThan(0);
    expect(domain.feeRateMax).toBeGreaterThan(domain.feeRateMin);
    expect(domain.cyclesMin).toBeGreaterThan(0);
    expect(domain.cyclesMax).toBeGreaterThan(domain.cyclesMin);
  });

  it('maps larger transactions to larger bubbles', () => {
    const domain = buildMetricDomain([
      { size: 300, feeRate: 8, cycles: 20_000 },
      { size: 6_000, feeRate: 8, cycles: 20_000 },
    ]);

    const small = mapTxToScatterPoint({ size: 300, feeRate: 8, cycles: 20_000 }, domain);
    const large = mapTxToScatterPoint({ size: 6_000, feeRate: 8, cycles: 20_000 }, domain);

    expect(large.radius).toBeGreaterThan(small.radius);
  });

  it('marks missing feeRate and cycles', () => {
    const domain = buildMetricDomain([{ size: 500, feeRate: 20, cycles: 10_000 }]);
    const point = mapTxToScatterPoint({ size: 500 }, domain);

    expect(point.missingFeeRate).toBe(true);
    expect(point.missingCycles).toBe(true);
    expect(point.x).toBe(0);
    expect(point.y).toBe(1);
  });
});
