import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { render } from '../utils/test-utils';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScript: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptCapacityChart: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCodeCells: vi.fn(),
    getCellsByScriptRef: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: 'SECP256K1_BLAKE160' }),
  useSearchParams: () => new URLSearchParams(),
}));

const olderCodeHash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const newerCodeHash = '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const olderDataHash = '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
const newerDataHash = '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd';
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
    dataHash: olderDataHash,
    typeHash: olderCodeHash,
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
    dataHash: newerDataHash,
    typeHash: newerCodeHash,
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
  usedCapacitySum: '6100000000',
  liveUsedCapacitySum: '6100000000',
  byDeployment: [
    {
      codeHash: olderCodeHash,
      scriptKind: 'lock',
      cellsCount: 2,
      liveCellsCount: 1,
      capacitySum: '2000000000',
      liveCapacitySum: '1000000000',
      usedCapacitySum: '1200000000',
      liveUsedCapacitySum: '610000000',
    },
    {
      codeHash: newerCodeHash,
      scriptKind: 'lock',
      cellsCount: 8,
      liveCellsCount: 7,
      capacitySum: '8000000000',
      liveCapacitySum: '9000000000',
      usedCapacitySum: '4900000000',
      liveUsedCapacitySum: '5490000000',
    },
  ],
};

const mockCapacityChart = {
  title: 'SECP256K1_BLAKE160 Capacity History',
  series: [
    { key: 'used', label: 'Used', color: '#f59e0b' },
    { key: 'unused', label: 'Unused', color: '#00c389' },
  ],
  data: [
    {
      date: '2024-01-15',
      values: { used: '6000000000', unused: '4000000000' },
    },
  ],
};

const emptyCells = {
  data: [],
  total: 0,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

const mockCodeCells = {
  codeCells: [
    {
      txHash: newerCodeCellTxHash,
      outputIndex: 1,
      status: 'live' as const,
      createdAtBlock: 12345,
      capacity: '16200000000',
    },
  ],
  liveCount: 1,
  totalCount: 1,
};

describe('ScriptDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getScript).mockResolvedValue(mockDeployments);
    vi.mocked(api.getScriptUsage).mockResolvedValue(mockUsage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getScriptCapacityChartByCodeHash).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getCodeCells).mockResolvedValue(mockCodeCells);
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(emptyCells);
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [olderCodeHash]: {
        codeHash: olderCodeHash,
        name: 'SECP256K1_BLAKE160',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: olderCodeHash,
        deploymentDataHash: olderDataHash,
        codeCellTxHash: olderCodeCellTxHash,
        codeCellOutputIndex: 0,
        liveCellsCount: 1,
        liveCapacitySum: '1000000000',
        liveUsedCapacitySum: '610000000',
        codeCellsLiveCount: 1,
        codeCellsTotal: 1,
      },
      [newerCodeHash]: {
        codeHash: newerCodeHash,
        name: 'SECP256K1_BLAKE160',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: newerCodeHash,
        deploymentDataHash: newerDataHash,
        codeCellTxHash: newerCodeCellTxHash,
        codeCellOutputIndex: 1,
        liveCellsCount: 7,
        liveCapacitySum: '9000000000',
        liveUsedCapacitySum: '5490000000',
        codeCellsLiveCount: 1,
        codeCellsTotal: 1,
      },
    });
  });

  it('prioritizes deployments and renders selected deployment detail sections', async () => {
    const user = userEvent.setup();

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScriptCapacityChartByCodeHash).toHaveBeenCalledWith(newerCodeHash, 'lock');
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: newerCodeHash,
        hashType: 'type',
        scriptKind: 'lock',
        limit: 50,
        cursor: undefined,
      });
    });

    expect(screen.getByText('Deployments')).toBeInTheDocument();
    expect(screen.getByText('Deployed At')).toBeInTheDocument();
    expect(screen.queryByTestId('script-ref-semantics')).not.toBeInTheDocument();
    expect(screen.getByText('References')).toBeInTheDocument();
    expect(screen.getByText('Code Cells')).toBeInTheDocument();
    expect(screen.getByText('Usage')).toBeInTheDocument();
    expect(screen.queryByText('Capacity Statistics')).not.toBeInTheDocument();
    expect(screen.queryByText('1 live')).not.toBeInTheDocument();
    expect(screen.queryByText('Deployment 1')).not.toBeInTheDocument();
    expect(screen.queryByText('Deployment 2')).not.toBeInTheDocument();

    const olderDeploymentRow = screen.getByTestId(`deployment-row-${olderCodeHash}-type`);
    expect(
      within(olderDeploymentRow).getAllByTitle(`Click to copy: ${olderCodeHash}`).length
    ).toBeGreaterThan(0);

    await user.click(
      within(olderDeploymentRow).getAllByTitle(`Click to copy: ${olderCodeHash}`)[0]
    );

    await waitFor(() => {
      expect(api.getScriptCapacityChartByCodeHash).toHaveBeenCalledTimes(1);
      expect(api.getCellsByScriptRef).toHaveBeenCalledTimes(1);
      expect(api.getCodeCells).toHaveBeenCalledTimes(1);
    });

    await user.click(olderDeploymentRow);

    await waitFor(() => {
      expect(api.getScriptCapacityChartByCodeHash).toHaveBeenCalledWith(olderCodeHash, 'lock');
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: olderCodeHash,
        hashType: 'type',
        scriptKind: 'lock',
        limit: 50,
        cursor: undefined,
      });
      expect(api.getCodeCells).toHaveBeenCalledWith(olderCodeHash, 'type');
    });
  });
});
