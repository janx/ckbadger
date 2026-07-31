import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import MinerAddressDistributionPage from '@/app/charts/miner-address-distribution/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getMinerAddressDistributionChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('MinerAddressDistributionPage', () => {
  it('shows the exact window and distinguishes addresses from lock hashes', async () => {
    const minerAddress = `ckb1${'a'.repeat(42)}`;
    const unresolvedHash = `0x${'b'.repeat(64)}`;
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue({
      title: 'Miner Distribution (Last 7 Complete Days, UTC+8)',
      totalBlocks: 10,
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

    expect(
      await screen.findByText('Miner Distribution (Last 7 Complete Days, UTC+8)')
    ).toBeInTheDocument();
    expect(screen.getByText('2026-07-23–2026-07-29 · Total Blocks: 10')).toBeInTheDocument();
    expect(screen.getByText('Miner / Lock Hash')).toBeInTheDocument();
    expect(screen.getAllByText('Resolved Pool')).not.toHaveLength(0);

    const unresolvedLink = screen.getByRole('link', { name: /0xbbbbbbbb/ });
    expect(unresolvedLink).toHaveAttribute('href', `/mainnet/address/${unresolvedHash.slice(2)}`);
  });
});
