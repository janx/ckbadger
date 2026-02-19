import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptDetailPage from '@/app/scripts/[name]/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScript: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptOccupationChart: vi.fn(),
    getScriptOccupationChartByCodeHash: vi.fn(),
    getCellsByScriptRef: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('next/navigation', () => ({
  useParams: () => ({ name: 'SECP256K1_BLAKE160' }),
}));

const mockCodeHash = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';

const mockDeployments = [
  {
    codeHash: mockCodeHash,
    name: 'SECP256K1_BLAKE160',
    description: 'Default lock script',
    scriptKind: 'lock',
    rfc: null,
    website: null,
    sourceUrl: null,
    decoderType: null,
    network: 'mainnet',
    hashType: 'type',
    dataHash: null,
    typeHash: null,
    tag: null,
    deprecated: false,
    isSystem: true,
    codeCellTxHash: null,
    codeCellOutputIndex: null,
  },
];

const mockUsage = {
  name: 'SECP256K1_BLAKE160',
  cellsCount: 10,
  liveCellsCount: 8,
  capacitySum: '10000000000',
  liveCapacitySum: '10000000000',
  occupiedCapacitySum: '6100000000',
  liveOccupiedCapacitySum: '6100000000',
  byDeployment: [
    {
      codeHash: mockCodeHash,
      scriptKind: 'lock',
      cellsCount: 10,
      liveCellsCount: 8,
      capacitySum: '10000000000',
      liveCapacitySum: '10000000000',
      occupiedCapacitySum: '6100000000',
      liveOccupiedCapacitySum: '6100000000',
    },
  ],
};

const mockOccupationChart = {
  title: 'SECP256K1_BLAKE160 Capacity Occupation',
  series: [
    { key: 'occupied', label: 'Occupied', color: '#f59e0b' },
    { key: 'unoccupied', label: 'Unoccupied', color: '#00c389' },
  ],
  data: [
    {
      date: '2024-01-15',
      values: { occupied: '6000000000', unoccupied: '4000000000' },
    },
  ],
};

const emptyCells = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getScript).mockResolvedValue(mockDeployments);
    vi.mocked(api.getScriptUsage).mockResolvedValue(mockUsage);
    vi.mocked(api.getScriptOccupationChart).mockResolvedValue(mockOccupationChart);
    vi.mocked(api.getScriptOccupationChartByCodeHash).mockResolvedValue(mockOccupationChart);
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(emptyCells);
  });

  it('renders capacity utilization blocks for script and selected deployment', async () => {
    render(<ScriptDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Capacity Utilization')).toBeInTheDocument();
      expect(screen.getByText('Selected Deployment Utilization')).toBeInTheDocument();
      expect(screen.getByText('Occupation History')).toBeInTheDocument();
    });

    expect(screen.getAllByText(/^Occupied:/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/^Unoccupied:/).length).toBeGreaterThan(0);
  });
});
