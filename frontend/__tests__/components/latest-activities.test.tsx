import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestActivities } from '@/components/latest-activities';
import type { GlobalActivity } from '@/lib/api';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getLatestActivities: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
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
    hasTypeScript: overrides.hasTypeScript ?? false,
    assetChanges: overrides.assetChanges ?? [],
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    protocolActions: overrides.protocolActions ?? [],
    peers: overrides.peers ?? [],
  };
}

describe('LatestActivities stream', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders each activity as a separate stream item (no tx grouping)', async () => {
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

    await waitFor(() => {
      const addressLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/address/'));
      expect(addressLinks).toHaveLength(2);
    });
  });

  it('renders a DAO deposit with the DAO Deposit label', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qdao111111111111111111111111111111111111111111111',
        txHash: '0xtx-dao',
        ckbDelta: '-10200000000',
        assetChanges: [{ type: 'daoDeposit', capacity: '10200000000' }],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/DAO Deposit/)).toBeInTheDocument();
    });
  });

  it('renders a token transfer with the token symbol', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qtoken1111111111111111111111111111111111111111111',
        txHash: '0xtx-token',
        assetChanges: [
          { type: 'token', typeScriptHash: '0xtoken', delta: '1200', symbol: 'SEAL', decimals: 8 },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/SEAL Transfer/)).toBeInTheDocument();
    });
  });

  it('renders explicit tx and block links without nesting inner detail links', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qtoken1111111111111111111111111111111111111111111',
        txHash: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
        blockNumber: 12_345,
        assetChanges: [
          { type: 'token', typeScriptHash: '0xtoken', delta: '1200', symbol: 'SEAL', decimals: 8 },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      const txLink = screen
        .getAllByRole('link')
        .find(
          (link) =>
            link.getAttribute('href') ===
            '/tx/0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef'
        );
      const blockLink = screen
        .getAllByRole('link')
        .find((link) => link.getAttribute('href') === '/blocks/12345');
      const addressLink = screen
        .getAllByRole('link')
        .find(
          (link) =>
            link.getAttribute('href') ===
            '/address/ckb1qtoken1111111111111111111111111111111111111111111'
        );
      const tokenLink = screen
        .getAllByRole('link')
        .find((link) => link.getAttribute('href') === '/tokens/0xtoken');

      expect(txLink).toBeTruthy();
      expect(blockLink).toBeTruthy();
      expect(addressLink).toBeTruthy();
      expect(tokenLink).toBeTruthy();
      expect(document.querySelector('a a')).toBeNull();
    });
  });

  it('renders a generic type script call label with the script name', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qscript1111111111111111111111111111111111111111111',
        txHash: '0xtx-script',
        typeCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Omnilock',
          },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/Script Call \(type\)/)).toBeInTheDocument();
      expect(screen.getAllByText(/Omnilock/).length).toBeGreaterThan(0);
      expect(screen.queryByText(/Type call/)).not.toBeInTheDocument();
    });
  });

  it('keeps the generic type script call label when scriptName is set', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qprotocol111111111111111111111111111111111111111',
        txHash: '0xtx-protocol',
        typeCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Stable++ Pool',
          },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/Script Call \(type\)/)).toBeInTheDocument();
      expect(screen.getAllByText(/Stable\+\+ Pool/).length).toBeGreaterThan(0);
      expect(screen.queryByText(/Type call/)).not.toBeInTheDocument();
    });
  });

  it('removes the hash-type prefix from type script refs in the stream', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qscriptref1111111111111111111111111111111111111111',
        txHash: '0xtx-script-ref',
        typeCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'data1',
            typeArgs: '0x1234',
            scriptHash: '0x1234567890abcdef',
          },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/Script Call \(type\)/)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: '0x12345678' })).toBeInTheDocument();
      expect(screen.queryByText('data1:0x12345678')).not.toBeInTheDocument();
    });
  });

  it('renders CKB transfer for activities with no assets and no script calls', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qtransfer11111111111111111111111111111111111111111',
        txHash: '0xtx-ckb',
        ckbDelta: '-50000000000',
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/CKB Transfer/)).toBeInTheDocument();
    });
  });

  it('renders a protocol action lock call with script name', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qlockaction111111111111111111111111111111111111',
        txHash: '0xtx-lock',
        lockCalls: [
          {
            lockCodeHash: '0xintent',
            lockHashType: 'type',
            lockArgs: '0xargs',
            scriptHash: '0xhash',
            scriptName: 'UTXOSwap Intent',
          },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      // Script name appears twice: once as protocol label, once as link text in LockCallExpr
      expect(screen.getAllByText(/UTXOSwap Intent/).length).toBeGreaterThanOrEqual(1);
    });
  });

  it('limits visible items to 20', async () => {
    const activities = Array.from({ length: 25 }, (_, i) =>
      makeActivity({
        address: `ckb1qaddr${String(i).padStart(40, '0')}`,
        txHash: `0xtx-${i}`,
        blockNumber: 11_000 - i,
        ckbDelta: '100000000',
      })
    );

    vi.mocked(api.getLatestActivities).mockResolvedValue(activities);

    render(<LatestActivities />);

    await waitFor(() => {
      const addressLinks = screen
        .getAllByRole('link')
        .filter((link) => link.getAttribute('href')?.startsWith('/address/'));
      expect(addressLinks).toHaveLength(20);
    });
  });

  it('shows skeleton while loading', () => {
    vi.mocked(api.getLatestActivities).mockReturnValue(new Promise(() => {}));

    render(<LatestActivities />);

    expect(screen.getByTestId('latest-activities-content')).toBeInTheDocument();
  });

  it('supports page mode without the self-referential view-all link', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qpage1111111111111111111111111111111111111111111',
        txHash: '0xtx-page',
        ckbDelta: '100000000',
      }),
    ]);

    render(<LatestActivities queryLimit={64} maxItems={64} showViewAllLink={false} scrollable />);

    await waitFor(() => {
      expect(api.getLatestActivities).toHaveBeenCalledWith(64);
    });
    expect(screen.queryByRole('link', { name: /view all/i })).not.toBeInTheDocument();
    expect(screen.getByTestId('latest-activities-content')).toHaveClass('overflow-y-auto');
  });
});
