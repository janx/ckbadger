import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScript: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptCapacityChart: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCellsByScriptRef: vi.fn(),
    lookupScripts: vi.fn(),
  },
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
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getScript).mockResolvedValue(mockDeployments);
    vi.mocked(api.getScriptUsage).mockResolvedValue(mockUsage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getScriptCapacityChartByCodeHash).mockResolvedValue(mockCapacityChart);
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
      },
    });
  });

  it('renders separate capacity and cells sections for the latest deployment', async () => {
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScriptCapacityChartByCodeHash).toHaveBeenCalledWith(newerCodeHash, 'lock');
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: newerCodeHash,
        hashType: 'type',
        scriptKind: 'lock',
        limit: 20,
        cursor: undefined,
      });
    });

    expect(screen.getByText('Capacity Statistics')).toBeInTheDocument();
    expect(screen.getByText('Deployed At')).toBeInTheDocument();
    expect(screen.getAllByText('Cells').length).toBeGreaterThan(0);
    expect(screen.queryByText('Capacity History')).not.toBeInTheDocument();
    expect(screen.queryByText('Selected Deployment Utilization')).not.toBeInTheDocument();
    const refSemantics = screen.getByTestId('script-ref-semantics');
    expect(refSemantics).toBeInTheDocument();
    expect(
      within(refSemantics).getByRole('link', { name: 'Reference doc: data vs type hash semantics' })
    ).toHaveAttribute('href', 'https://docs.nervos.org/docs/tech-explanation/data-type-diff');
    const capacityRefs = screen.getByTestId('capacity-selected-refs');
    const cellsRefs = screen.getByTestId('cells-selected-refs');
    expect(within(capacityRefs).getByText('type')).toBeInTheDocument();
    expect(within(capacityRefs).getByText('bytecode(data)')).toBeInTheDocument();
    expect(within(cellsRefs).getByText('type')).toBeInTheDocument();
    expect(within(cellsRefs).getByText('bytecode(data)')).toBeInTheDocument();

    const codeCellLinks = screen
      .getAllByRole('link')
      .map((link) => link.getAttribute('href'))
      .filter((href): href is string => href?.startsWith('/cell/') ?? false);
    expect(codeCellLinks).toContain(`/cell/${newerCodeCellTxHash}-1`);
    expect(codeCellLinks).toContain(`/cell/${olderCodeCellTxHash}-0`);
  });
});
