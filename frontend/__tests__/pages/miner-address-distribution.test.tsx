import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import MinerAddressDistributionPage from '@/app/charts/miner-address-distribution/page';
import { api } from '@/lib/api';
import { fullText } from '@/__tests__/utils/full-text';

vi.mock('@/lib/api', () => ({
  api: {
    getMinerAddressDistributionChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('MinerAddressDistributionPage', () => {
  it('shows the exact window and distinguishes resolved addresses from lock hashes', async () => {
    const minerAddress = `ckb1${'a'.repeat(42)}`;
    const unresolvedHash = `0x${'b'.repeat(64)}`;
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue({
      title: 'Miner Distribution (Last 7 Complete Days, UTC+8)',
      totalBlocks: 10,
      restBlocks: 0,
      windowDays: 7,
      fromDate: '2026-07-23',
      toDate: '2026-07-29',
      data: [
        {
          minerLockHash: `0x${'c'.repeat(64)}`,
          address: minerAddress,
          minerName: 'Resolved Pool',
          blocksMined: 6,
          percentage: '60.0000',
        },
        {
          minerLockHash: unresolvedHash,
          address: null,
          minerName: null,
          blocksMined: 4,
          percentage: '40.0000',
        },
      ],
    });
    render(<MinerAddressDistributionPage />);
    expect(await screen.findByText('Miners')).toBeInTheDocument();
    // Re-pinned for CH-03: the window with an arrow; a lock hash shortened like an address, its
    // kind said once under the ranking instead of a bracketed word on each row.
    // re-pinned (A4 round 2): the range and the count are units of their own in the line
    expect(screen.getByText(fullText('2026-07-23 → 2026-07-29 · 10 blocks'))).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Resolved Pool' })).toHaveAttribute(
      'href',
      `/mainnet/address/${minerAddress}`
    );
    const unresolved = screen.getByRole('link', { name: `0x${'b'.repeat(8)}…${'b'.repeat(6)}` });
    expect(unresolved).toHaveAttribute('href', `/mainnet/address/${unresolvedHash.slice(2)}`);
    expect(unresolved).toHaveAttribute('title', unresolvedHash);
    expect(screen.queryByText('(Address)')).toBeNull();
    expect(
      screen.getByText('A row beginning 0x is a miner known only by its lock hash.')
    ).toBeInTheDocument();
  });
});
