import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-03: each Miners row printed the full 97-character address, then a
// literal "(Address)" that broke as "(Addr / ess)", then the value on another line; the summary
// "Leading miner: ckb1qzda0c… / 93hhsj (67.8320%)" broke at the ellipsis; the window read
// "2026-11-07-2026-11-13". R9 (re-pinned): the shortened address is `ckb1…` and its last 8
// characters.

vi.mock('@/lib/api', () => ({
  api: {
    getMinerAddressDistributionChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

const address =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq0tpsqq08mkay9ewrfdwlcghv62qw704s93hhsj';
const lockHash = `0x${'b'.repeat(64)}`;

describe('Miners ranking (CH-03)', () => {
  beforeEach(() => {
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue({
      title: 'Miner Distribution',
      totalBlocks: 61726,
      windowDays: 7,
      fromDate: '2026-11-07',
      toDate: '2026-11-13',
      data: [
        {
          minerLockHash: `0x${'a'.repeat(64)}`,
          address,
          minerName: null,
          blocksMined: 41870,
          percentage: '67.8320',
        },
        {
          minerLockHash: lockHash,
          address: null,
          minerName: null,
          blocksMined: 8548,
          percentage: '13.8482',
        },
      ],
    });
  });

  it('writes each row as rank, the shortened address as a link and the value, no kind word', async () => {
    render(<ChartDetail slug="miner-address-distribution" />);
    const ranking = await screen.findByRole('list', { name: 'Miners ranking' });
    const [first, second] = within(ranking).getAllByRole('listitem');
    const link = within(first).getByRole('link', { name: 'ckb1…4s93hhsj' });
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(link).toHaveAttribute('title', address);
    expect(first).toHaveTextContent(/^1\. ckb1…4s93hhsj41,870 blocks · 67\.83%$/);
    expect(second).toHaveTextContent(/^2\. 0xbbbbbbbb…bbbbbb8,548 blocks · 13\.85%$/);
    expect(document.body).not.toHaveTextContent(/\((Address|Lock hash)\)/);
    // The full address is in the link's title only, never printed.
    expect(screen.queryByText(address)).toBeNull();
    // A miner known only by its lock hash is said in words once, quietly, under the ranking.
    expect(screen.getAllByText(/lock hash/i)).toHaveLength(1);
  });

  // Re-pinned for lane D1 (the chart-hero board): the leading miner is named, whole, in the
  // figure's eyebrow over its window (was a "Leading miner" fact under the lede); the fact with its
  // share stands in the evidence tier.
  it('keeps the leading miner whole and writes the window with an arrow', async () => {
    render(<ChartDetail slug="miner-address-distribution" />);
    await screen.findByRole('list', { name: 'Miners ranking' });
    expect(screen.getByTestId('figure-eyebrow')).toHaveTextContent(
      'ckb1…4s93hhsj, the largest miner · 2026-11-07 → 2026-11-13'
    );
    expect(screen.getByText(/^Leading miner:/)).toHaveTextContent(
      'Leading miner: ckb1…4s93hhsj · 41,870 blocks · 67.83%'
    );
    expect(document.body).toHaveTextContent('2026-11-07 → 2026-11-13');
    expect(document.body).not.toHaveTextContent(/2026-11-07[-–]2026-11-13/);
  });
});
