import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import producerMainnet from '@/__tests__/fixtures/network-api-producer/hardforks-mainnet.json';
import producerDeployments from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';

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
  consensusFeatures: [
    { rfc: '0029', activationEpoch: 0 },
    { rfc: '0032', activationEpoch: 5414 },
    { rfc: '0049', activationEpoch: 12293 },
  ],
  scriptDeployments: [],
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

  it('renders the mainnet consensus producer payload at its unsynced tip', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(producerMainnet);
    render(<HardforksPage />);
    expect(await screen.findByText('CKB Edition Mirana')).toBeInTheDocument();
    expect(screen.getByText('RFC 0049')).toBeInTheDocument();
    expect(screen.getByText(/mainnet · tip epoch 0/)).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /View activation block/ })).not.toBeInTheDocument();
  });

  it('renders timeline rows from API data', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(mockTimeline);

    render(<HardforksPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Hardforks' })).toBeInTheDocument();

    await waitFor(() => {
      expect(getHardforksMock).toHaveBeenCalled();
      expect(screen.getByText('CKB Edition Meepo')).toBeInTheDocument();
    });

    expect(screen.getByText(/mainnet · tip epoch 13,000/)).toBeInTheDocument();
    expect(screen.getByRole('table', { name: /VM version by hash type/i })).toBeInTheDocument();
    expect(screen.getByText('RFC 0032')).toBeInTheDocument();
    expect(screen.getByText('CKB Edition Mirana')).toBeInTheDocument();
    expect(screen.getAllByText('activated').length).toBeGreaterThanOrEqual(2);
    const activationLinks = screen.getAllByRole('link', { name: /View activation block/ });
    expect(activationLinks.length).toBe(2);
    expect(activationLinks[0]).toHaveAttribute('href', '/mainnet/blocks/18430000');
    expect(activationLinks[1]).toHaveAttribute('href', '/mainnet/blocks/8775638');
    expect(screen.getByRole('link', { name: 'CKB2023' })).toHaveAttribute(
      'href',
      'https://example.com/ckb2023'
    );
  });

  it('shows the producer-backed script deployment mark and exact evidence links', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue(producerDeployments);
    render(<HardforksPage />);

    const mark = await screen.findByRole('button', { name: /Known Lock v2.*block #10/i });
    fireEvent.focus(mark);
    expect(
      within(
        screen.getByRole('complementary', { name: 'Selected script deployment evidence' })
      ).getByText(/earliest indexed code-cell creation/i)
    ).toBeInTheDocument();
    // Header time in the site's one UTC clock form, not a raw ISO string (#16).
    expect(screen.getByText('2023-11-14 22:13:20 UTC')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Known Lock family' })).toHaveAttribute(
      'href',
      '/mainnet/scripts/Known%20Lock'
    );
    expect(screen.getByRole('link', { name: 'Code cell 0x55555555…:0' })).toHaveAttribute(
      'href',
      '/mainnet/cell/0x5555555555555555555555555555555555555555555555555555555555555555-0'
    );
    expect(screen.getByRole('link', { name: 'Block #10' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/10'
    );
  });

  it('shows error state on fetch failure', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockRejectedValue(new Error('network down'));

    render(<HardforksPage />);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Failed to load hardfork timeline');
    });
  });
});
