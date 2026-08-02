import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { CKBytesCard } from '@/components/ckbytes-card';
import type { NetworkStats } from '@/lib/api';

/** Whole CKB → shannon string (no BigInt literals: tsconfig targets < ES2020). */
function ckb(whole: number): string {
  return `${whole}00000000`;
}

function mockStats(overrides: Partial<NetworkStats> = {}): NetworkStats {
  return {
    latestBlock: 20032104,
    avgBlockTime: '9.0s',
    hashRate: '72.98 PH/s',
    difficulty: '2.34 P',
    epoch: '13500(150/1800)',
    tps: '1.23',
    estimatedEpochTime: '4h',
    transactionsPerMinute: '12',
    transactionsPerDay: '17280',
    syncStatus: {
      isSyncing: false,
      syncedBlock: 20032104,
      tipBlock: 20032104,
      progress: 100,
      estimatedTime: null,
      chartDataMayBeIncomplete: false,
      blocksPerSecond: null,
      emaBlocksPerSecond: null,
      syncMode: 'normal',
      startedAt: null,
      elapsedTime: null,
      totalTime: null,
    },
    deepForkStatus: {
      detected: false,
      detectedAt: null,
      depth: null,
      dbTip: null,
      chainTip: null,
      forkPoint: null,
    },
    // Mainnet magnitudes: 49.3B circulating, 159.7M common knowledge, 8.37B in DAO.
    knowledgeSize: ckb(159_690_777),
    circulatingSupply: ckb(49_345_845_077),
    daoLocked: ckb(8_371_195_163),
    ...overrides,
  };
}

describe('CKBytesCard', () => {
  it('allocates the free segment as the exact remainder of circulating supply', () => {
    render(<CKBytesCard stats={mockStats()} />);

    // free = 49,345,845,077 − 159,690,777 − 8,371,195,163 = 40,814,959,137 CKB.
    // With the pre-fix raw-U knowledge size (5.2B CKB) this read 35.77B.
    expect(screen.getByTitle(/^Free: 40\.81B CKB/)).toBeInTheDocument();
    expect(screen.getByTitle(/^Knowledge: 159\.69M CKB/)).toBeInTheDocument();
    expect(screen.getByTitle(/^DAO: 8\.37B CKB/)).toBeInTheDocument();
  });

  it('surfaces an impossible allocation instead of clamping the free segment to zero', () => {
    render(
      <CKBytesCard
        stats={mockStats({
          knowledgeSize: ckb(40_000_000_000),
          daoLocked: ckb(20_000_000_000),
          circulatingSupply: ckb(49_345_845_077),
        })}
      />
    );

    // The old Math.max(0, …) clamp painted a plausible bar over a broken state.
    expect(screen.queryByTitle(/^Free: 0 CKB/)).not.toBeInTheDocument();
    const error = screen.getByTestId('ckbytes-allocation-error');
    expect(error.textContent).toContain('40,000,000,000');
    expect(error.textContent).toContain('20,000,000,000');
    expect(error.textContent).toContain('49,345,845,077');
  });
});
