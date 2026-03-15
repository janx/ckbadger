import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TokenDetailPage from '@/app/tokens/[typeHash]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getToken: vi.fn(),
    getTokenCapacityChart: vi.fn(),
    getTokenHolders: vi.fn(),
    getTokenActivities: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  useParams: () => ({
    typeHash: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
  }),
  useSearchParams: () => new URLSearchParams(),
}));

const mockToken = {
  typeScriptHash: '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
  typeCodeHash: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
  typeHashType: 'type',
  typeArgs: '0x00',
  standard: 'xudt',
  name: 'Test Token',
  symbol: 'TEST',
  decimals: 8,
  description: 'A test token',
  iconUrl: null,
  published: false,
  famous: false,
  tags: null,
  udtType: null,
  manager: null,
  email: null,
  operatorWebsite: null,
  totalSupply: '1000000000000',
  maximumSupply: null,
  maximumSupplyStatus: 'unknown' as const,
  holdersCount: 42,
  transfersCount: 1000,
  transfers24h: 10,
  cellsCount: 150,
  totalCapacity: '50000000000000',
  totalUsedCapacity: '15300000000000',
};

const mockHolders = {
  data: [],
  total: 0,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

const mockActivities = {
  data: [],
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('TokenDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getTokenCapacityChart).mockResolvedValue({
      title: 'TEST Capacity History',
      series: [
        { key: 'used', label: 'Used', color: '#f59e0b' },
        { key: 'unused', label: 'Unused', color: '#00c389' },
      ],
      data: [
        {
          date: '2024-01-15',
          values: { used: '15300000000000', unused: '34700000000000' },
        },
      ],
    });
    vi.mocked(api.getTokenHolders).mockResolvedValue(mockHolders);
    vi.mocked(api.getTokenActivities).mockResolvedValue(mockActivities);
  });

  it('renders token overview and capacity statistics', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(
      <TokenDetailPage typeHash="0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef" />
    );

    await waitFor(() => {
      expect(screen.getByText('TEST')).toBeInTheDocument();
    });

    expect(screen.getByText('XUDT')).toBeInTheDocument();
    expect(screen.getByText('A test token')).toBeInTheDocument();
    expect(screen.getByText('Total Circulation')).toBeInTheDocument();
    expect(screen.getByText('Maximum Supply')).toBeInTheDocument();
    expect(screen.getByText('Unknown')).toBeInTheDocument();
    expect(screen.getByText('Capacity Statistics')).toBeInTheDocument();
    expect(screen.getByText('Cells Capacity')).toBeInTheDocument();
    expect(screen.getByText(/^Used:/)).toBeInTheDocument();
    expect(screen.getByText(/^Unused:/)).toBeInTheDocument();
    expect(screen.getAllByText('Cells').length).toBeGreaterThan(0);
    expect(screen.getByText('150')).toBeInTheDocument();
  });

  it('renders unlimited max supply when status is unlimited', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      standard: 'sudt',
      maximumSupplyStatus: 'unlimited',
    });

    render(
      <TokenDetailPage typeHash="0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef" />
    );

    await waitFor(() => {
      expect(screen.getByText('Unlimited')).toBeInTheDocument();
    });
  });

  it('defaults to activities tab and renders transfer details', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);
    vi.mocked(api.getTokenActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xabcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234',
          blockNumber: 12345,
          txIndex: 0,
          timestamp: '1700000000000',
          actions: ['mint', 'transfer'],
          transfers: [
            {
              fromLockHash: null,
              fromAddress: null,
              toLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
              toAddress: null,
              amount: '100000000000',
              isMint: true,
              isBurn: false,
            },
            {
              fromLockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
              fromAddress: null,
              toLockHash: '0x2222222222222222222222222222222222222222222222222222222222222222',
              toAddress: null,
              amount: '50000000000',
              isMint: false,
              isBurn: false,
            },
          ],
        },
      ],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(
      <TokenDetailPage typeHash="0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef" />
    );

    await waitFor(() => {
      expect(screen.getByText('Mint')).toBeInTheDocument();
    });

    expect(screen.getAllByText('Activities').length).toBeGreaterThan(0);
    expect(screen.getByText('mint')).toBeInTheDocument();
    expect(screen.getByText('transfer')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '#12,345' })).toHaveAttribute('href', '/blocks/12345');
    expect(
      screen
        .getAllByRole('link')
        .some(
          (link) =>
            link.getAttribute('href') ===
            '/tx/0xabcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234'
        )
    ).toBe(true);
  });
});
