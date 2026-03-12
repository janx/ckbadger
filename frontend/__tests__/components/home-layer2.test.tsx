import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { KnowledgeSizeTrend, NetworkHealth, ScriptUtilization } from '@/components/home-layer2';
import { api, type ChartResponse, type NetworkStats } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getKnowledgeSizeChart: vi.fn(),
    getAverageBlockTimeChart: vi.fn(),
    getHashRateChart: vi.fn(),
    getScripts: vi.fn(),
  },
}));

function mockChartResponse(overrides: Partial<ChartResponse> = {}): ChartResponse {
  return {
    title: 'Test Chart',
    yAxisLabel: 'Value',
    data: Array.from({ length: 30 }, (_, i) => ({
      date: `2026-02-${String(i + 1).padStart(2, '0')}`,
      value: String(100 + i * 10),
    })),
    ...overrides,
  };
}

function mockNetworkStats(): NetworkStats {
  return {
    latestBlock: 15000000,
    avgBlockTime: '8.20s',
    hashRate: '1.23 EH/s',
    difficulty: '0x1234567890',
    epoch: '5000',
    tps: '3.5',
    estimatedEpochTime: '14400',
    transactionsPerMinute: '12',
    transactionsPerDay: '17280',
    syncStatus: {
      isSyncing: false,
      syncedBlock: 15000000,
      tipBlock: 15000000,
      progress: 100,
      estimatedTime: null,
      chartDataMayBeIncomplete: false,
      blocksPerSecond: null,
      emaBlocksPerSecond: null,
      syncMode: 'live',
      startedAt: null,
      elapsedTime: null,
      totalTime: null,
    },
    deepForkStatus: {
      detected: false,
      detectedAt: null,
      depth: null,
      dbTip: null,
      chainTip: null,
      forkPoint: null,
    },
    knowledgeSize: '19500000000.00',
    circulatingSupply: '25200000000.00',
    daoLocked: '11200000000.00',
  };
}

describe('KnowledgeSizeTrend', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders with "Knowledge Size" text', async () => {
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue(mockChartResponse());

    render(<KnowledgeSizeTrend />);

    await waitFor(() => {
      expect(screen.getByText('Knowledge Size — Daily Change')).toBeInTheDocument();
    });
  });

  it('renders delta bar chart after loading', async () => {
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue(mockChartResponse());

    const { container } = render(<KnowledgeSizeTrend />);

    await waitFor(() => {
      // Delta bar chart renders div bars with title attributes
      const bars = container.querySelectorAll('[title]');
      expect(bars.length).toBeGreaterThan(0);
    });
  });
});

describe('NetworkHealth', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders with "Block Time" and "Hash Rate" text', async () => {
    vi.mocked(api.getAverageBlockTimeChart).mockResolvedValue(mockChartResponse());
    vi.mocked(api.getHashRateChart).mockResolvedValue(mockChartResponse());

    render(<NetworkHealth stats={mockNetworkStats()} />);

    await waitFor(() => {
      expect(screen.getByText('Block Time')).toBeInTheDocument();
    });

    expect(screen.getByText('Hash Rate')).toBeInTheDocument();
  });

  it('shows current block time value from stats', async () => {
    vi.mocked(api.getAverageBlockTimeChart).mockResolvedValue(mockChartResponse());
    vi.mocked(api.getHashRateChart).mockResolvedValue(mockChartResponse());

    render(<NetworkHealth stats={mockNetworkStats()} />);

    await waitFor(() => {
      expect(screen.getByText('8.20s')).toBeInTheDocument();
    });
  });

  it('renders with null stats gracefully', async () => {
    vi.mocked(api.getAverageBlockTimeChart).mockResolvedValue(mockChartResponse());
    vi.mocked(api.getHashRateChart).mockResolvedValue(mockChartResponse());

    render(<NetworkHealth stats={null} />);

    await waitFor(() => {
      expect(screen.getByText('Block Time')).toBeInTheDocument();
    });

    expect(screen.getByText('Hash Rate')).toBeInTheDocument();
  });
});

describe('ScriptUtilization', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders with "Script Utilization" text', async () => {
    vi.mocked(api.getScripts).mockResolvedValue({
      data: [
        {
          codeHash: '0xaaa',
          name: 'secp256k1',
          description: null,
          scriptKind: null,
          rfc: null,
          website: null,
          sourceUrl: null,
          decoderType: null,
          network: 'mainnet',
          hashType: null,
          dataHash: null,
          typeHash: null,
          tag: null,
          deprecated: false,
          isSystem: true,
          codeCellTxHash: null,
          codeCellOutputIndex: null,
          liveUsedCapacitySum: '5000000000',
          liveCellsCount: 100000,
        },
        {
          codeHash: '0xbbb',
          name: 'dao',
          description: null,
          scriptKind: null,
          rfc: null,
          website: null,
          sourceUrl: null,
          decoderType: null,
          network: 'mainnet',
          hashType: null,
          dataHash: null,
          typeHash: null,
          tag: null,
          deprecated: false,
          isSystem: true,
          codeCellTxHash: null,
          codeCellOutputIndex: null,
          liveUsedCapacitySum: '3000000000',
          liveCellsCount: 50000,
        },
      ],
      total: 2,
      limit: 5,
      hasMore: false,
      nextCursor: null,
    });

    render(<ScriptUtilization />);

    await waitFor(() => {
      expect(screen.getByText('secp256k1')).toBeInTheDocument();
    });

    expect(screen.getByText('dao')).toBeInTheDocument();
    expect(screen.getByText('Script Utilization')).toBeInTheDocument();
  });

  it('shows loading state initially', () => {
    vi.mocked(api.getScripts).mockReturnValue(new Promise(() => {}));

    const { container } = render(<ScriptUtilization />);

    const pulseElements = container.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThanOrEqual(1);
  });
});
