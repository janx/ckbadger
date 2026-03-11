import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ChartsPage from '@/app/charts/page';
import { api, MostUtilizedAssetsChartResponse, MostUtilizedScriptsChartResponse } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getNetworkStats: vi.fn(),
    getDaoTotalDepositChart: vi.fn(),
    getDaoDailyDepositChart: vi.fn(),
    getDaoCirculationRatioChart: vi.fn(),
    getTransactionCountChart: vi.fn(),
    getCellCountChart: vi.fn(),
    getKnowledgeSizeChart: vi.fn(),
    getCommonKnowledgeCompositionChart: vi.fn(),
    getCellAgeVsUsedCapacityChart: vi.fn(),
    getCapacityTurnoverRatioChart: vi.fn(),
    getCellSizeDistributionChart: vi.fn(),
    getAddressCohortRetentionChart: vi.fn(),
    getMostUtilizedScriptsChart: vi.fn(),
    getMostUtilizedAssetsChart: vi.fn(),
    getBlockTimeDistributionChart: vi.fn(),
    getEpochTimeDistributionChart: vi.fn(),
    getAverageBlockTimeChart: vi.fn(),
    getEpochTimeLengthChart: vi.fn(),
    getHashRateChart: vi.fn(),
    getDifficultyChart: vi.fn(),
    getUncleRateChart: vi.fn(),
    getMinerAddressDistributionChart: vi.fn(),
    getTotalSupplyChart: vi.fn(),
    getNominalApcChart: vi.fn(),
    getSecondaryIssuanceChart: vi.fn(),
    getInflationRateChart: vi.fn(),
    getHodlWaveChart: vi.fn(),
    getHardforks: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: ({ isPercentage }: { isPercentage?: boolean }) => (
    <div data-testid={isPercentage ? 'stacked-area-percentage' : 'stacked-area-absolute'} />
  ),
}));

const mockNetworkStatsComplete = {
  latestBlock: 1000000,
  avgBlockTime: '8.5',
  hashRate: '1.5 PH/s',
  difficulty: '0x1000000000',
  epoch: '100/50/1000',
  tps: '2.5',
  estimatedEpochTime: '4h 30m',
  transactionsPerMinute: '150',
  transactionsPerDay: '216000',
  syncStatus: {
    isSyncing: false,
    syncedBlock: 1000000,
    tipBlock: 1000000,
    progress: 100,
    estimatedTime: null,
    chartDataMayBeIncomplete: false,
    blocksPerSecond: null,
    emaBlocksPerSecond: null,
    syncMode: 'synced',
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
  knowledgeSize: null,
  circulatingSupply: null,
  daoLocked: null,
};

const mockNetworkStatsSyncing = {
  ...mockNetworkStatsComplete,
  syncStatus: {
    isSyncing: true,
    syncedBlock: 500000,
    tipBlock: 1000000,
    progress: 50,
    estimatedTime: '2h 30m',
    chartDataMayBeIncomplete: true,
    blocksPerSecond: 1500.0,
    emaBlocksPerSecond: 1200.0,
    syncMode: 'bulk',
    startedAt: 1700000000,
    elapsedTime: '1h 15m',
    totalTime: null,
  },
};

const mockChartResponse = {
  title: 'Test Chart',
  data: [{ date: '2024-01-01', value: '100' }],
  yAxisLabel: 'Value',
};

const mockMinerDistributionResponse = {
  title: 'Miner Distribution',
  data: [{ address: 'ckb1...', minerName: 'Pool', blocksMined: 100, percentage: '50' }],
  totalBlocks: 200,
};

const mockStackedAreaResponse = {
  title: 'Total Supply',
  data: [{ date: '2024-01-01', values: { primary: '100', secondary: '50', dao: '25' } }],
  series: [
    { key: 'primary', label: 'Primary', color: '#4ade80' },
    { key: 'secondary', label: 'Secondary', color: '#818cf8' },
    { key: 'dao', label: 'DAO', color: '#f472b6' },
  ],
};

const mockCellCountResponse = {
  title: 'Cell Count',
  data: [{ date: '2024-01-01', values: { allCells: '1000', liveCells: '800', deadCells: '200' } }],
  series: [
    { key: 'allCells', label: 'All Cells', color: '#6b7280' },
    { key: 'liveCells', label: 'Live Cells', color: '#00c389' },
    { key: 'deadCells', label: 'Dead Cells', color: '#ef4444' },
  ],
};

const mockMostUtilizedScriptsResponse: MostUtilizedScriptsChartResponse = {
  title: 'Scripts Used & Total CKBytes',
  usedShare: {
    title: 'Top Scripts Used Share',
    data: [{ date: '2024-01-01', values: { top0: '100', others: '20' } }],
    series: [
      { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
      { key: 'others', label: 'Others', color: '#64748b' },
    ],
  },
  capacityShare: {
    title: 'Top Scripts Capacity Share',
    data: [{ date: '2024-01-01', values: { top0: '150', others: '30' } }],
    series: [
      { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
      { key: 'others', label: 'Others', color: '#64748b' },
    ],
  },
};

const mockMostUtilizedAssetsResponse: MostUtilizedAssetsChartResponse = {
  title: 'Assets Used & Total CKBytes',
  usedShare: {
    title: 'Top Assets Used Share',
    data: [{ date: '2024-01-01', values: { top0: '120', others: '40' } }],
    series: [
      { key: 'top0', label: 'CKBTEST (token)', color: '#00c389' },
      { key: 'others', label: 'Others', color: '#64748b' },
    ],
  },
  capacityShare: {
    title: 'Top Assets Capacity Share',
    data: [{ date: '2024-01-01', values: { top0: '180', others: '60' } }],
    series: [
      { key: 'top0', label: 'CKBTEST (token)', color: '#00c389' },
      { key: 'others', label: 'Others', color: '#64748b' },
    ],
  },
};

const mockHardforkTimeline = {
  network: 'mainnet',
  tipEpoch: 13000,
  tipBlock: 19000000,
  events: [
    {
      id: 'mirana-2021',
      name: 'CKB Edition Mirana',
      shortName: 'Mirana',
      editionYear: 2021,
      activationEpoch: 5414,
      activationDate: '2022-05-10',
      activationBlock: 8775638,
      status: 'activated' as const,
      summary: 'CKB-VM v1 activation.',
      resources: [],
    },
    {
      id: 'meepo-2024',
      name: 'CKB Edition Meepo',
      shortName: 'Meepo',
      editionYear: 2024,
      activationEpoch: 12293,
      activationDate: '2025-07-01',
      activationBlock: 18430000,
      status: 'activated' as const,
      summary: 'CKB-VM v2 activation.',
      resources: [],
    },
  ],
};

describe('ChartsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getDaoDailyDepositChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getDaoCirculationRatioChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getTransactionCountChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getCellCountChart).mockResolvedValue(mockCellCountResponse);
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getCommonKnowledgeCompositionChart).mockResolvedValue(mockStackedAreaResponse);
    vi.mocked(api.getCellAgeVsUsedCapacityChart).mockResolvedValue(mockStackedAreaResponse);
    vi.mocked(api.getCapacityTurnoverRatioChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getCellSizeDistributionChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getAddressCohortRetentionChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue(mockMostUtilizedScriptsResponse);
    vi.mocked(api.getMostUtilizedAssetsChart).mockResolvedValue(mockMostUtilizedAssetsResponse);
    vi.mocked(api.getBlockTimeDistributionChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getEpochTimeDistributionChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getAverageBlockTimeChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getEpochTimeLengthChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getHashRateChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getDifficultyChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getUncleRateChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue(
      mockMinerDistributionResponse
    );
    vi.mocked(api.getTotalSupplyChart).mockResolvedValue(mockStackedAreaResponse);
    vi.mocked(api.getNominalApcChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getSecondaryIssuanceChart).mockResolvedValue(mockStackedAreaResponse);
    vi.mocked(api.getInflationRateChart).mockResolvedValue(mockChartResponse);
    vi.mocked(api.getHodlWaveChart).mockResolvedValue(mockStackedAreaResponse);
    vi.mocked(api.getHardforks).mockResolvedValue(mockHardforkTimeline);
  });

  it('renders the page with header and title', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(mockNetworkStatsComplete);

    render(<ChartsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Charts')).toBeInTheDocument();
  });

  it('shows warning when chartDataMayBeIncomplete is true', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(mockNetworkStatsSyncing);

    render(<ChartsPage />);

    await waitFor(() => {
      expect(screen.getByText(/Chart data may be incomplete/i)).toBeInTheDocument();
    });
  });

  it('renders chart sections', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(mockNetworkStatsComplete);

    render(<ChartsPage />);

    expect(screen.getByText('Proof of Work')).toBeInTheDocument();
    expect(screen.getByText('Nervos DAO')).toBeInTheDocument();
    expect(screen.getByText('Block')).toBeInTheDocument();
    expect(screen.getByText('Activities')).toBeInTheDocument();
    expect(screen.getByText('Common Knowledge Bytes')).toBeInTheDocument();
    expect(screen.getByText('Economics')).toBeInTheDocument();
    expect(screen.getByText('Scripts Used & Total CKBytes')).toBeInTheDocument();
    expect(screen.getByText('Assets Used & Total CKBytes')).toBeInTheDocument();

    expect(screen.queryByText('Hardfork Markers on Epoch Time Length')).not.toBeInTheDocument();
  });

  it('uses percentage mode in overview previews that are percentage charts', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(mockNetworkStatsComplete);

    render(<ChartsPage />);

    await waitFor(() => {
      expect(screen.getAllByTestId('stacked-area-percentage')).toHaveLength(4);
      expect(screen.getAllByTestId('stacked-area-absolute')).toHaveLength(3);
    });
  });
});
