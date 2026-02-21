import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TokenDetailPage from '@/app/tokens/[typeHash]/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getToken: vi.fn(),
    getTokenOccupationChart: vi.fn(),
    getTokenHolders: vi.fn(),
    getTokenTransfers: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('next/navigation', () => ({
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
  totalOccupiedCapacity: '15300000000000',
};

const mockHolders = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const mockTransfers = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('TokenDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getTokenOccupationChart).mockResolvedValue({
      title: 'TEST Capacity Occupation',
      series: [
        { key: 'occupied', label: 'Occupied', color: '#f59e0b' },
        { key: 'unoccupied', label: 'Unoccupied', color: '#00c389' },
      ],
      data: [
        {
          date: '2024-01-15',
          values: { occupied: '15300000000000', unoccupied: '34700000000000' },
        },
      ],
    });
    vi.mocked(api.getTokenHolders).mockResolvedValue(mockHolders);
    vi.mocked(api.getTokenTransfers).mockResolvedValue(mockTransfers);
  });

  it('renders cells count stat', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Cells')).toBeInTheDocument();
      expect(screen.getByText('150')).toBeInTheDocument();
    });
  });

  it('renders cells capacity inside capacity utilization', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.queryByText('Capacity Snapshot')).not.toBeInTheDocument();
      expect(screen.getByText('Cells Capacity')).toBeInTheDocument();
    });
  });

  it('renders capacity utilization bar', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.queryByText('Capacity Utilization')).not.toBeInTheDocument();
      expect(screen.getByText(/^Occupied:/)).toBeInTheDocument();
      expect(screen.getByText(/\(\d+\.\d% occupied\)/)).toBeInTheDocument();
    });
  });

  it('renders occupied and unoccupied breakdown', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.getByText(/^Occupied:/)).toBeInTheDocument();
      expect(screen.getByText(/^Unoccupied:/)).toBeInTheDocument();
    });
  });

  it('renders basic token info', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('TEST')).toBeInTheDocument();
      expect(screen.getByText('XUDT')).toBeInTheDocument();
      expect(screen.getByText('A test token')).toBeInTheDocument();
    });
  });

  it('renders circulation label and unknown max supply for xUDT without cap observation', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Total Circulation')).toBeInTheDocument();
      expect(screen.getByText('Maximum Supply')).toBeInTheDocument();
      expect(screen.getByText('Unknown')).toBeInTheDocument();
    });
  });

  it('renders unlimited max supply when status is unlimited', async () => {
    vi.mocked(api.getToken).mockResolvedValue({
      ...mockToken,
      standard: 'sudt',
      maximumSupplyStatus: 'unlimited',
    });

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Unlimited')).toBeInTheDocument();
    });
  });

  it('renders occupation history panel', async () => {
    vi.mocked(api.getToken).mockResolvedValue(mockToken);

    render(<TokenDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Capacity & Occupation')).toBeInTheDocument();
    });
  });
});
