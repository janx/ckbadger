import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api } from '@/lib/api';
import { formatCapacity } from '@/lib/utils';

vi.mock('@/lib/api', () => ({
  api: {
    getScriptFamilyDetail: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptCapacityChart: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCell: vi.fn(),
    getCellsByScriptRef: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: 'SECP256K1_BLAKE160' }),
  useSearchParams: () => new URLSearchParams(),
}));

const sharedVersionCodeHash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const legacyVersionCodeHash = '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const firstDeploymentDataHash =
  '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
const secondDeploymentDataHash =
  '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd';
const legacyDeploymentDataHash =
  '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee';
const sharedObservedData1Reference =
  '0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff';
const firstDeploymentAt = Date.parse('2024-01-01T00:00:00.000Z');
const secondDeploymentAt = Date.parse('2024-02-01T00:00:00.000Z');
const legacyDeploymentAt = Date.parse('2023-12-01T00:00:00.000Z');
const firstDeploymentTxHash = '0x1111111111111111111111111111111111111111111111111111111111111111';
const secondDeploymentTxHash = '0x2222222222222222222222222222222222222222222222222222222222222222';
const legacyDeploymentTxHash = '0x3333333333333333333333333333333333333333333333333333333333333333';
const firstDeploymentBlock = 12344;
const secondDeploymentBlock = 12345;
const legacyDeploymentBlock = 12000;
const firstDeploymentTimestampLabel = new Date(firstDeploymentAt).toLocaleString();
const secondDeploymentTimestampLabel = new Date(secondDeploymentAt).toLocaleString();
const legacyDeploymentTimestampLabel = new Date(legacyDeploymentAt).toLocaleString();
const firstGovernanceCodeHash =
  '0x0000000000000000000000000000000000000000000000000000000000000000';
const secondGovernanceCodeHash =
  '0xf222222222222222222222222222222222222222222222222222222222222222';
const legacyGovernanceCodeHash =
  '0xf333333333333333333333333333333333333333333333333333333333333333';
const firstGovernanceArgs = '0xa111111111111111111111111111111111111111';
const secondGovernanceArgs = '0xa222222222222222222222222222222222222222';
const legacyGovernanceArgs = '0xa333333333333333333333333333333333333333';
const secondGovernanceScriptName = 'Secp256k1Blake160';

const mockScriptFamilyDetail = {
  familyId: 'secp256k1-blake160',
  name: 'SECP256K1_BLAKE160',
  description: 'Default lock script',
  scriptKind: 'lock',
  website: null,
  liveCellsCount: 8,
  cellsCount: 10,
  ownedCapacitySum: '10000000000',
  ownedKnowledgeSum: '6100000000',
  versionsCount: 2,
  versions: [
    {
      versionHash: sharedVersionCodeHash,
      name: 'SECP256K1_BLAKE160',
      description: 'Default lock script',
      scriptKind: 'lock',
      website: null,
      deprecated: false,
      canonicalReferenceHash: sharedVersionCodeHash,
      canonicalHashType: 'type',
      deployedAt: firstDeploymentAt,
      liveCellsCount: 7,
      cellsCount: 8,
      ownedCapacitySum: '9000000000',
      ownedKnowledgeSum: '5490000000',
      codeCellsLiveCount: 2,
      codeCellsTotal: 2,
      deployments: [
        {
          hashType: 'type',
          typeReferenceHash: sharedVersionCodeHash,
          dataReferenceHash: firstDeploymentDataHash,
          codeCellTxHash: firstDeploymentTxHash,
          codeCellOutputIndex: 0,
          deployedAt: firstDeploymentAt,
        },
        {
          hashType: 'data',
          typeReferenceHash: sharedVersionCodeHash,
          dataReferenceHash: secondDeploymentDataHash,
          codeCellTxHash: secondDeploymentTxHash,
          codeCellOutputIndex: 1,
          deployedAt: secondDeploymentAt,
        },
      ],
      references: [
        {
          referenceHash: sharedVersionCodeHash,
          hashType: 'type',
          liveCellsCount: 4,
          cellsCount: 6,
          ownedCapacitySum: '700',
          ownedKnowledgeSum: '400',
        },
        {
          referenceHash: sharedObservedData1Reference,
          hashType: 'data1',
          liveCellsCount: 3,
          cellsCount: 4,
          ownedCapacitySum: '300',
          ownedKnowledgeSum: '200',
        },
      ],
    },
    {
      versionHash: legacyVersionCodeHash,
      name: 'SECP256K1_BLAKE160',
      description: 'Default lock script',
      scriptKind: 'lock',
      website: null,
      deprecated: false,
      canonicalReferenceHash: legacyVersionCodeHash,
      canonicalHashType: 'type',
      deployedAt: legacyDeploymentAt,
      liveCellsCount: 1,
      cellsCount: 2,
      ownedCapacitySum: '1000000000',
      ownedKnowledgeSum: '610000000',
      codeCellsLiveCount: 0,
      codeCellsTotal: 1,
      deployments: [
        {
          hashType: 'type',
          typeReferenceHash: legacyVersionCodeHash,
          dataReferenceHash: legacyDeploymentDataHash,
          codeCellTxHash: legacyDeploymentTxHash,
          codeCellOutputIndex: 0,
          deployedAt: legacyDeploymentAt,
        },
      ],
      references: [
        {
          referenceHash: legacyVersionCodeHash,
          hashType: 'type',
          liveCellsCount: 1,
          cellsCount: 2,
          ownedCapacitySum: '1000000000',
          ownedKnowledgeSum: '610000000',
        },
      ],
    },
  ],
};

const mockUsage = {
  name: 'SECP256K1_BLAKE160',
  cellsCount: 10,
  liveCellsCount: 8,
  capacitySum: '10000000000',
  ownedCapacitySum: '10000000000',
  commonKnowledgeSizeSum: '6100000000',
  ownedKnowledgeSum: '6100000000',
  byDeployment: [
    {
      codeHash: sharedVersionCodeHash,
      scriptKind: 'lock',
      cellsCount: 8,
      liveCellsCount: 7,
      capacitySum: '8000000000',
      ownedCapacitySum: '9000000000',
      commonKnowledgeSizeSum: '4900000000',
      ownedKnowledgeSum: '5490000000',
    },
    {
      codeHash: legacyVersionCodeHash,
      scriptKind: 'lock',
      cellsCount: 2,
      liveCellsCount: 1,
      capacitySum: '2000000000',
      ownedCapacitySum: '1000000000',
      commonKnowledgeSizeSum: '1200000000',
      ownedKnowledgeSum: '610000000',
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

const sharedVersionUsageCellsByRef = {
  type: {
    data: [
      {
        txHash: '0x4444444444444444444444444444444444444444444444444444444444444444',
        outputIndex: 0,
        capacity: '10000000000',
        lockScriptHash: '0xlive1',
        dataSize: 0,
        createdAtBlock: 12340,
      },
    ],
    total: 1,
    limit: 50,
    hasMore: false,
    nextCursor: null,
  },
  firstData: {
    data: [
      {
        txHash: '0x5555555555555555555555555555555555555555555555555555555555555555',
        outputIndex: 1,
        capacity: '11000000000',
        lockScriptHash: '0xlive2',
        dataSize: 16,
        createdAtBlock: 12341,
      },
    ],
    total: 1,
    limit: 50,
    hasMore: false,
    nextCursor: null,
  },
  secondData: {
    data: [
      {
        txHash: '0x6666666666666666666666666666666666666666666666666666666666666666',
        outputIndex: 2,
        capacity: '12000000000',
        lockScriptHash: '0xlive3',
        dataSize: 32,
        createdAtBlock: 12342,
      },
    ],
    total: 1,
    limit: 50,
    hasMore: false,
    nextCursor: null,
  },
};

function hasTextContent(element: HTMLElement, text: string): boolean {
  return element.textContent?.replace(/\s+/g, '').includes(text.replace(/\s+/g, '')) ?? false;
}

function setViewportWidth(width: number): void {
  Object.defineProperty(window, 'innerWidth', {
    configurable: true,
    writable: true,
    value: width,
  });
  window.dispatchEvent(new Event('resize'));
}

describe('ScriptDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setViewportWidth(1280);
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(mockScriptFamilyDetail as any);
    vi.mocked(api.getScriptUsage).mockResolvedValue(mockUsage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getScriptCapacityChartByCodeHash).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getCell).mockImplementation(async (txHash, outputIndex) => {
      if (txHash === firstDeploymentTxHash && outputIndex === 0) {
        return {
          txHash,
          outputIndex,
          capacity: '16100000000',
          commonKnowledgeSize: 6100000000,
          lockScriptHash: '0xlock1',
          dataSize: 0,
          createdAtBlock: firstDeploymentBlock,
          status: 'live',
          lock: {
            codeHash: firstGovernanceCodeHash,
            hashType: 'type',
            args: firstGovernanceArgs,
          },
        };
      }

      if (txHash === secondDeploymentTxHash && outputIndex === 1) {
        return {
          txHash,
          outputIndex,
          capacity: '16200000000',
          commonKnowledgeSize: 6200000000,
          lockScriptHash: '0xlock2',
          dataSize: 0,
          createdAtBlock: secondDeploymentBlock,
          status: 'live',
          lock: {
            codeHash: secondGovernanceCodeHash,
            hashType: 'type',
            args: secondGovernanceArgs,
          },
        };
      }

      if (txHash === legacyDeploymentTxHash && outputIndex === 0) {
        return {
          txHash,
          outputIndex,
          capacity: '16000000000',
          commonKnowledgeSize: 6000000000,
          lockScriptHash: '0xlock3',
          dataSize: 0,
          createdAtBlock: legacyDeploymentBlock,
          status: 'dead',
          consumedAtBlock: legacyDeploymentBlock + 1,
          lock: {
            codeHash: legacyGovernanceCodeHash,
            hashType: 'type',
            args: legacyGovernanceArgs,
          },
        };
      }

      throw new Error(`unexpected outpoint ${txHash}:${outputIndex}`);
    });
    vi.mocked(api.getCellsByScriptRef).mockImplementation(async ({ codeHash, hashType }) => {
      if (codeHash === sharedVersionCodeHash && hashType === 'type') {
        return sharedVersionUsageCellsByRef.type;
      }

      if (codeHash === firstDeploymentDataHash && hashType === 'data') {
        return sharedVersionUsageCellsByRef.firstData;
      }

      if (codeHash === secondDeploymentDataHash && hashType === 'data') {
        return sharedVersionUsageCellsByRef.secondData;
      }

      if (codeHash === sharedObservedData1Reference && hashType === 'data1') {
        return emptyCells;
      }

      return emptyCells;
    });
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [sharedVersionCodeHash]: {
        codeHash: sharedVersionCodeHash,
        name: 'SECP256K1_BLAKE160',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: sharedVersionCodeHash,
        deploymentDataHash: firstDeploymentDataHash,
        codeCellTxHash: firstDeploymentTxHash,
        codeCellOutputIndex: 0,
        liveCellsCount: 7,
        ownedCapacitySum: '9000000000',
        ownedKnowledgeSum: '5490000000',
        codeCellsLiveCount: 2,
        codeCellsTotal: 2,
      },
      [legacyVersionCodeHash]: {
        codeHash: legacyVersionCodeHash,
        name: 'SECP256K1_BLAKE160',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: legacyVersionCodeHash,
        deploymentDataHash: legacyDeploymentDataHash,
        codeCellTxHash: legacyDeploymentTxHash,
        codeCellOutputIndex: 0,
        liveCellsCount: 1,
        ownedCapacitySum: '1000000000',
        ownedKnowledgeSum: '610000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 1,
      },
      [secondGovernanceCodeHash]: {
        codeHash: secondGovernanceCodeHash,
        name: secondGovernanceScriptName,
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: secondGovernanceCodeHash,
        deploymentDataHash: null,
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 0,
        ownedCapacitySum: '0',
        ownedKnowledgeSum: '0',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
      },
    });
  });

  it('keeps compact tables on tablet widths instead of switching to cards', async () => {
    setViewportWidth(900);

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScriptFamilyDetail).toHaveBeenCalledWith('SECP256K1_BLAKE160');
      expect(screen.getByText('SECP256K1_BLAKE160')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('script-versions-compact')).toBeNull();
    expect(screen.queryByTestId('version-deployments-compact')).toBeNull();

    expect(screen.getByRole('button', { name: 'Explain Script Versions' })).toBeInTheDocument();
    expect(screen.getByText('Code Hash')).toBeInTheDocument();
    expect(screen.getByText('First Deployed At')).toBeInTheDocument();
    expect(screen.getByText('Deployments')).toBeInTheDocument();
    expect(screen.getByText('Cells Using It')).toBeInTheDocument();
    expect(screen.getByText('Capacity Using It')).toBeInTheDocument();
    expect(screen.queryByText('Used As')).toBeNull();

    const sharedVersionRow = screen.getByTestId(`version-row-${sharedVersionCodeHash}`);
    expect(within(sharedVersionRow).getByText('LOCK')).toBeInTheDocument();

    expect(screen.getByText('Outpoint')).toBeInTheDocument();
    expect(screen.getByText('Governance')).toBeInTheDocument();
    expect(screen.getByText('References')).toBeInTheDocument();
    expect(screen.getByText('Deployed At')).toBeInTheDocument();
    expect(screen.getByText('Common Knowledge Size')).toBeInTheDocument();
    expect(screen.queryByText('Status')).toBeNull();
    expect(screen.getByRole('button', { name: 'Explain Version Deployments' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Explain References' })).toBeInTheDocument();

    const firstDeploymentRow = screen.getByTestId(`deployment-row-${firstDeploymentTxHash}:0`);
    expect(within(firstDeploymentRow).getByText('Live')).toBeInTheDocument();
    expect(within(firstDeploymentRow).getByText('Immutable (all-zero lock)')).toBeInTheDocument();
  });

  it('switches to cards on mobile widths', async () => {
    setViewportWidth(640);

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScriptFamilyDetail).toHaveBeenCalledWith('SECP256K1_BLAKE160');
      expect(screen.getByText('SECP256K1_BLAKE160')).toBeInTheDocument();
    });

    const compactVersions = screen.getByTestId('script-versions-compact');
    expect(
      within(compactVersions).getByTitle(`Click to copy: ${sharedVersionCodeHash}`)
    ).toBeInTheDocument();
    expect(within(compactVersions).getAllByText('First deployed').length).toBeGreaterThan(0);
    expect(within(compactVersions).getAllByText('Cells using it').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Explain Script Versions' })).toBeInTheDocument();
    expect(screen.queryByText('Code Hash')).toBeNull();
    expect(screen.queryByText('Capacity Using It')).toBeNull();

    const compactDeployments = screen.getByTestId('version-deployments-compact');
    expect(within(compactDeployments).getAllByText('Governance').length).toBeGreaterThan(0);
    expect(within(compactDeployments).getByText('Immutable (all-zero lock)')).toBeInTheDocument();
    expect(within(compactDeployments).getAllByText('Common knowledge size').length).toBeGreaterThan(
      0
    );
    expect(screen.getByRole('button', { name: 'Explain Version Deployments' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Explain References' })).toBeNull();
    expect(screen.queryByText('Status')).toBeNull();
    expect(screen.queryByText('Deployed At')).toBeNull();
    expect(screen.queryByText('Common Knowledge Size')).toBeNull();
  });

  it('shows deprecated badges for known deprecated scripts in the header and tables', async () => {
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue({
      ...mockScriptFamilyDetail,
      name: 'PW Lock',
      description: 'Ethereum wallet compatible lock',
      versions: mockScriptFamilyDetail.versions.map((version) => ({
        ...version,
        name: 'PW Lock',
        description: 'Ethereum wallet compatible lock',
        deprecated: true,
      })),
    } as any);
    vi.mocked(api.getScriptUsage).mockResolvedValue({
      ...mockUsage,
      name: 'PW Lock',
    });

    render(<ScriptDetailPage name="PW Lock" />);

    await waitFor(() => {
      expect(screen.getByText('PW Lock')).toBeInTheDocument();
    });

    expect(screen.queryByText('Unlabeled Script')).toBeNull();
    expect(screen.getAllByText('Deprecated').length).toBeGreaterThan(1);
  });

  it('renders zero-deployment versions without synthesizing fake deployment rows', async () => {
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue({
      ...mockScriptFamilyDetail,
      versions: [
        {
          ...mockScriptFamilyDetail.versions[0],
          deployments: [],
          codeCellsLiveCount: 0,
          codeCellsTotal: 0,
        },
      ],
      versionsCount: 1,
    } as any);
    vi.mocked(api.getScriptUsage).mockResolvedValue({
      ...mockUsage,
      byDeployment: [
        {
          ...mockUsage.byDeployment[0],
          codeHash: sharedVersionCodeHash,
          liveCellsCount: 3,
          cellsCount: 4,
        },
      ],
    });

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScriptFamilyDetail).toHaveBeenCalledWith('SECP256K1_BLAKE160');
      expect(screen.getByText('SECP256K1_BLAKE160')).toBeInTheDocument();
    });

    expect(screen.queryByText('Script not found')).toBeNull();
    const versionRow = screen.getByTestId(`version-row-${sharedVersionCodeHash}`);
    expect(within(versionRow).getByText(/^0$/)).toBeInTheDocument();
    expect(screen.getByText('No deployments found for this version')).toBeInTheDocument();
  });

  it('derives the genesis special burn tooltip from the API value', async () => {
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue({
      data: [
        {
          txHash: '0x7777777777777777777777777777777777777777777777777777777777777777',
          outputIndex: 0,
          capacity: '840000000000000000',
          lockScriptHash: '0xburn',
          dataSize: 0,
          createdAtBlock: 0,
          cellType: 'genesis_special_burn',
          virtualCommonKnowledgeSize: '504000000000000000',
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    // The tooltip copy comes from the API value, so it stays truthful on every
    // network instead of quoting mainnet's genesis figure.
    expect(
      await screen.findByTitle(
        `Virtual common knowledge size: ${formatCapacity('504000000000000000')}`
      )
    ).toBeInTheDocument();
    expect(screen.queryByTitle(/5\.04B/)).toBeNull();
  });
});
