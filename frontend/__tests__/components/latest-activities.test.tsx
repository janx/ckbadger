import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestActivities } from '@/components/latest-activities';
import { api, type ActivityAssetChange, type GlobalActivity } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getLatestActivities: vi.fn(),
  },
}));

function makeActivity(
  overrides: Partial<GlobalActivity> & Pick<GlobalActivity, 'address' | 'txHash'>
): GlobalActivity {
  return {
    address: overrides.address,
    txHash: overrides.txHash,
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    assetChanges: overrides.assetChanges ?? [],
    peers: overrides.peers ?? [],
  };
}

function tokenChange(delta: string): ActivityAssetChange {
  return {
    type: 'token',
    typeScriptHash: '0xtoken',
    delta,
    symbol: 'SEAL',
    decimals: 8,
  };
}

describe('LatestActivities', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders one transaction block for multiple activities sharing a tx hash', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qsender11111111111111111111111111111111111111111',
        txHash: '0xtx-shared',
        ckbDelta: '-10000000000',
      }),
      makeActivity({
        address: 'ckb1qreceiver111111111111111111111111111111111111111',
        txHash: '0xtx-shared',
        ckbDelta: '10000000000',
      }),
    ]);

    const { container } = render(<LatestActivities />);

    await waitFor(() => {
      expect(container.querySelectorAll('a[href="/tx/0xtx-shared"]')).toHaveLength(1);
    });
  });

  it('renders only the first three participants and shows a more hint for the rest', async () => {
    const hiddenAddress = 'ckb1qhidden444444444444444444444444444444444444444';

    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qsender11111111111111111111111111111111111111111',
        txHash: '0xtx-many',
        ckbDelta: '-10000000000',
      }),
      makeActivity({
        address: 'ckb1qsender22222222222222222222222222222222222222222',
        txHash: '0xtx-many',
        ckbDelta: '-5000000000',
      }),
      makeActivity({
        address: 'ckb1qreceiver333333333333333333333333333333333333333',
        txHash: '0xtx-many',
        ckbDelta: '14900000000',
      }),
      makeActivity({
        address: hiddenAddress,
        txHash: '0xtx-many',
        ckbDelta: '100000000',
      }),
    ]);

    const { container } = render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('+1 more')).toBeInTheDocument();
    });

    expect(
      container.querySelector(
        'a[href="/address/ckb1qsender11111111111111111111111111111111111111111"]'
      )
    ).toBeTruthy();
    expect(
      container.querySelector(
        'a[href="/address/ckb1qsender22222222222222222222222222222222222222222"]'
      )
    ).toBeTruthy();
    expect(
      container.querySelector(
        'a[href="/address/ckb1qreceiver333333333333333333333333333333333333333"]'
      )
    ).toBeTruthy();
    expect(container.querySelector(`a[href="/address/${hiddenAddress}"]`)).toBeNull();
  });

  it('renders structural summaries for grouped transactions', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qsender11111111111111111111111111111111111111111',
        txHash: '0xtx-fallback',
        ckbDelta: '-10000000000',
        assetChanges: [tokenChange('-500')],
      }),
      makeActivity({
        address: 'ckb1qsender22222222222222222222222222222222222222222',
        txHash: '0xtx-fallback',
        ckbDelta: '-2000000000',
      }),
      makeActivity({
        address: 'ckb1qreceiver333333333333333333333333333333333333333',
        txHash: '0xtx-fallback',
        ckbDelta: '11900000000',
        assetChanges: [tokenChange('500')],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('2 sent · 1 received · 2 asset events')).toBeInTheDocument();
    });
  });

  it('renders dao summaries instead of structural fallback text', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qdao111111111111111111111111111111111111111111',
        txHash: '0xtx-dao',
        ckbDelta: '-10200000000',
        assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
      }),
      makeActivity({
        address: 'ckb1qpeer11111111111111111111111111111111111111111',
        txHash: '0xtx-dao',
        ckbDelta: '10200000000',
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('DAO deposit')).toBeInTheDocument();
    });
  });
});
