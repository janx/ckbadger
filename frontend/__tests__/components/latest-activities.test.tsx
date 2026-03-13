import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestActivities } from '@/components/latest-activities';
import {
  api,
  type ActivityAssetChange,
  type ActivityScriptCall,
  type GlobalActivity,
} from '@/lib/api';

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
    scriptCalls: overrides.scriptCalls ?? [],
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

function scriptCall(name = 'RGB++ Lock'): ActivityScriptCall {
  return {
    typeCodeHash: '0xcodehash',
    typeHashType: 'type',
    typeArgs: '0x1234abcd',
    scriptHash: '0xscript-hash',
    scriptName: name,
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

    render(<LatestActivities />);

    await waitFor(
      () => {
        expect(
          screen
            .getAllByRole('link')
            .filter((link) => link.getAttribute('href') === '/tx/0xtx-shared')
        ).toHaveLength(1);
      },
      {
        timeout: 3000,
      }
    );
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

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText('+1 more')).toBeInTheDocument();
    });

    expect(
      screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/address/'))
        .map((link) => link.getAttribute('href'))
    ).toEqual([
      '/address/ckb1qsender11111111111111111111111111111111111111111',
      '/address/ckb1qsender22222222222222222222222222222222222222222',
      '/address/ckb1qreceiver333333333333333333333333333333333333333',
    ]);
    expect(
      screen
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === `/address/${hiddenAddress}`)
    ).toBe(false);
  });

  it('renders script calls in a dedicated section separate from asset badges', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qscript1111111111111111111111111111111111111111',
        txHash: '0xtx-script-call',
        ckbDelta: '-100000000',
        assetChanges: [tokenChange('-500')],
        scriptCalls: [scriptCall()],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(
        screen.getByText('1 sent · 0 received · 1 asset event · 1 script call')
      ).toBeInTheDocument();
    });

    expect(screen.getByText('Assets')).toBeInTheDocument();
    expect(screen.getByText('Scripts')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'RGB++ Lock' })).toHaveAttribute(
      'href',
      '/scripts/RGB%2B%2B%20Lock'
    );
  });

  it('renders five activity groups and hides any additional groups', async () => {
    const txHashes = ['0xtx-1', '0xtx-2', '0xtx-3', '0xtx-4', '0xtx-5', '0xtx-6'];

    vi.mocked(api.getLatestActivities).mockResolvedValue(
      txHashes.map((txHash, index) =>
        makeActivity({
          address: `ckb1qgroup${index}11111111111111111111111111111111111111`,
          txHash,
          blockNumber: 11_000 - index,
          timestamp: String(1_700_000_000 - index),
          ckbDelta: '100000000',
        })
      )
    );

    render(<LatestActivities />);

    await waitFor(() => {
      expect(
        screen.getAllByRole('link').filter((link) => link.getAttribute('href')?.startsWith('/tx/'))
      ).toHaveLength(5);
    });

    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/tx/0xtx-5')
    ).toBe(true);
    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/tx/0xtx-6')
    ).toBe(false);
  });
});
