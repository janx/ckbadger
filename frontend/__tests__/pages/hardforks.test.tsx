import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';

const getHardforksMock = vi.hoisted(() => vi.fn());

vi.mock('@/lib/api', () => ({
  api: {
    getHardforks: getHardforksMock,
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockTimeline = {
  network: 'mainnet',
  tipEpoch: 13000,
  tipBlock: 19000000,
  events: [
    {
      id: 'meepo-2024',
      name: 'CKB Edition Meepo',
      shortName: 'Meepo',
      editionYear: 2024,
      activationEpoch: 12293,
      activationDate: '2025-07-01',
      activationBlock: 18430000,
      status: 'activated' as const,
      summary: 'VM v2 activation with Spawn syscall.',
      resources: [{ label: 'CKB2023', url: 'https://example.com/ckb2023' }],
    },
    {
      id: 'mirana-2021',
      name: 'CKB Edition Mirana',
      shortName: 'Mirana',
      editionYear: 2021,
      activationEpoch: 5414,
      activationDate: '2022-05-10',
      activationBlock: 8775638,
      status: 'activated' as const,
      summary: 'VM v1 activation and consensus patches.',
      resources: [{ label: 'CKB2021', url: 'https://example.com/ckb2021' }],
    },
  ],
};

describe('HardforksPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders timeline rows from API data', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);

    render(<HardforksPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('CKB Hardfork Timeline')).toBeInTheDocument();

    await waitFor(() => {
      expect(getHardforksMock).toHaveBeenCalled();
      expect(screen.getByText('CKB Edition Meepo')).toBeInTheDocument();
    });

    expect(screen.getByText(/Network: mainnet/)).toBeInTheDocument();
    expect(screen.getByText(/Tip epoch: 13,000/)).toBeInTheDocument();
    expect(screen.getByText(/Tip block: #19,000,000/)).toBeInTheDocument();
    expect(screen.getByText('CKB Edition Mirana')).toBeInTheDocument();
    expect(screen.getAllByText('ACTIVATED').length).toBeGreaterThanOrEqual(2);
    const activationLinks = screen.getAllByRole('link', { name: 'View activation block' });
    expect(activationLinks.length).toBe(2);
    expect(activationLinks[0]).toHaveAttribute('href', '/mainnet/blocks/18430000');
    expect(activationLinks[1]).toHaveAttribute('href', '/mainnet/blocks/8775638');
    expect(screen.getByRole('link', { name: 'CKB2023' })).toHaveAttribute(
      'href',
      'https://example.com/ckb2023'
    );
  });

  it('shows error state on fetch failure', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockRejectedValue(new Error('network down'));

    render(<HardforksPage />);

    await waitFor(() => {
      expect(screen.getByText('Failed to load hardfork timeline')).toBeInTheDocument();
    });
  });
});
