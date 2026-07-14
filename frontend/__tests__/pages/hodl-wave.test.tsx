import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import HodlWavePage from '@/app/charts/hodl-wave/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getHodlWaveChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockHodlWaveResponse = {
  title: 'CKB HODL Wave',
  data: [
    {
      date: '2024-01-01',
      values: {
        '24h': '5.00',
        '1d1w': '10.00',
        '1w1m': '15.00',
        '1m3m': '10.00',
        '3m6m': '10.00',
        '6m1y': '15.00',
        '1y3y': '20.00',
        gt3y: '15.00',
        holderCount: '42000',
      },
    },
  ],
  series: [
    { key: '24h', label: '24h', color: '#6366f1' },
    { key: '1d1w', label: '1d-1w', color: '#4ade80' },
    { key: '1w1m', label: '1w-1m', color: '#f87171' },
    { key: '1m3m', label: '1m-3m', color: '#f59e0b' },
    { key: '3m6m', label: '3m-6m', color: '#d4e157' },
    { key: '6m1y', label: '6m-1y', color: '#22c55e' },
    { key: '1y3y', label: '1y-3y', color: '#67e8f9' },
    { key: 'gt3y', label: '> 3y', color: '#a78bfa' },
  ],
};

describe('HodlWavePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders header and back link', () => {
    vi.mocked(api.getHodlWaveChart).mockResolvedValue(mockHodlWaveResponse);

    render(<HodlWavePage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('← Back to Charts')).toBeInTheDocument();
  });

  it('renders chart title', async () => {
    vi.mocked(api.getHodlWaveChart).mockResolvedValue(mockHodlWaveResponse);

    render(<HodlWavePage />);

    expect(screen.getByText('CKB HODL Wave')).toBeInTheDocument();
  });

  it('shows loading state initially', () => {
    vi.mocked(api.getHodlWaveChart).mockReturnValue(new Promise(() => {}));

    render(<HodlWavePage />);

    // Loading skeleton should be visible
    const skeleton = document.querySelector('.animate-pulse');
    expect(skeleton).toBeInTheDocument();
  });

  it('shows error state on failure', async () => {
    vi.mocked(api.getHodlWaveChart).mockRejectedValue(new Error('Network error'));

    render(<HodlWavePage />);

    await waitFor(() => {
      expect(screen.getByText('Failed to load chart data')).toBeInTheDocument();
    });
  });

  it('renders legend with all age bands and holder count', async () => {
    vi.mocked(api.getHodlWaveChart).mockResolvedValue(mockHodlWaveResponse);

    render(<HodlWavePage />);

    await waitFor(() => {
      expect(screen.getByText('24h')).toBeInTheDocument();
    });

    expect(screen.getByText('1d-1w')).toBeInTheDocument();
    expect(screen.getByText('1w-1m')).toBeInTheDocument();
    expect(screen.getByText('1m-3m')).toBeInTheDocument();
    expect(screen.getByText('3m-6m')).toBeInTheDocument();
    expect(screen.getByText('6m-1y')).toBeInTheDocument();
    expect(screen.getByText('1y-3y')).toBeInTheDocument();
    expect(screen.getByText('> 3y')).toBeInTheDocument();
    expect(screen.getByText('Holder Count')).toBeInTheDocument();
  });
});
