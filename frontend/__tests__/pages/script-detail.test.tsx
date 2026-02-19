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

const olderCodeHash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const newerCodeHash = '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const olderDeployedAt = Date.parse('2024-01-01T00:00:00.000Z');
const newerDeployedAt = Date.parse('2024-02-01T00:00:00.000Z');
const olderCodeCellTxHash = '0x1111111111111111111111111111111111111111111111111111111111111111';
const newerCodeCellTxHash = '0x2222222222222222222222222222222222222222222222222222222222222222';

const mockDeployments = [
  {
    codeHash: olderCodeHash,
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
    codeCellTxHash: olderCodeCellTxHash,
    codeCellOutputIndex: 0,
    deployedAt: olderDeployedAt,
  },
  {
    codeHash: newerCodeHash,
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
    codeCellTxHash: newerCodeCellTxHash,
    codeCellOutputIndex: 1,
    deployedAt: newerDeployedAt,
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
      codeHash: olderCodeHash,
      scriptKind: 'lock',
      cellsCount: 2,
      liveCellsCount: 1,
      capacitySum: '2000000000',
      liveCapacitySum: '1000000000',
      occupiedCapacitySum: '1200000000',
      liveOccupiedCapacitySum: '610000000',
    },
    {
      codeHash: newerCodeHash,
      scriptKind: 'lock',
      cellsCount: 8,
      liveCellsCount: 7,
      capacitySum: '8000000000',
      liveCapacitySum: '9000000000',
      occupiedCapacitySum: '4900000000',
      liveOccupiedCapacitySum: '5490000000',
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

  it('renders separate capacity and cells sections without occupation history section', async () => {
    render(<ScriptDetailPage />);

    await waitFor(() => {
      expect(screen.getAllByText('Capacity & Occupation').length).toBeGreaterThan(0);
    });

    expect(screen.getByText('Deployed At')).toBeInTheDocument();
    expect(screen.getAllByText('Cells').length).toBeGreaterThan(0);
    expect(screen.queryByText('Occupation History')).not.toBeInTheDocument();
    expect(screen.queryByText('Selected Deployment Utilization')).not.toBeInTheDocument();
    expect(screen.getAllByText(/^Occupied:/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/^Unoccupied:/).length).toBeGreaterThan(0);

    const codeCellLinks = Array.from(document.querySelectorAll('a[href^="/cell/"]')).map((link) =>
      link.getAttribute('href')
    );
    expect(codeCellLinks[0]).toBe(`/cell/${newerCodeCellTxHash}-1`);
    expect(codeCellLinks[1]).toBe(`/cell/${olderCodeCellTxHash}-0`);
  });
});
