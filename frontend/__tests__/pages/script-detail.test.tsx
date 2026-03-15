import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScript: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptCapacityChart: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCell: vi.fn(),
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

const sharedVersionCodeHash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const legacyVersionCodeHash = '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const firstDeploymentDataHash =
  '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
const secondDeploymentDataHash =
  '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd';
const legacyDeploymentDataHash =
  '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee';
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

const mockDeployments = [
  {
    codeHash: sharedVersionCodeHash,
    name: 'SECP256K1_BLAKE160',
    description: 'Default lock script',
    scriptKind: 'lock',
    rfc: null,
    website: null,
    sourceUrl: null,
    decoderType: null,
    network: 'mainnet',
    hashType: 'type',
    dataHash: firstDeploymentDataHash,
    typeHash: sharedVersionCodeHash,
    tag: null,
    deprecated: false,
    isSystem: true,
    codeCellTxHash: firstDeploymentTxHash,
    codeCellOutputIndex: 0,
    deployedAt: firstDeploymentAt,
  },
  {
    codeHash: sharedVersionCodeHash,
    name: 'SECP256K1_BLAKE160',
    description: 'Default lock script',
    scriptKind: 'lock',
    rfc: null,
    website: null,
    sourceUrl: null,
    decoderType: null,
    network: 'mainnet',
    hashType: 'data',
    dataHash: secondDeploymentDataHash,
    typeHash: sharedVersionCodeHash,
    tag: null,
    deprecated: false,
    isSystem: true,
    codeCellTxHash: secondDeploymentTxHash,
    codeCellOutputIndex: 1,
    deployedAt: secondDeploymentAt,
  },
  {
    codeHash: legacyVersionCodeHash,
    name: 'SECP256K1_BLAKE160',
    description: 'Default lock script',
    scriptKind: 'lock',
    rfc: null,
    website: null,
    sourceUrl: null,
    decoderType: null,
    network: 'mainnet',
    hashType: 'type',
    dataHash: legacyDeploymentDataHash,
    typeHash: legacyVersionCodeHash,
    tag: null,
    deprecated: false,
    isSystem: true,
    codeCellTxHash: legacyDeploymentTxHash,
    codeCellOutputIndex: 0,
    deployedAt: legacyDeploymentAt,
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
      codeHash: sharedVersionCodeHash,
      scriptKind: 'lock',
      cellsCount: 8,
      liveCellsCount: 7,
      capacitySum: '8000000000',
      liveCapacitySum: '9000000000',
      usedCapacitySum: '4900000000',
      liveUsedCapacitySum: '5490000000',
    },
    {
      codeHash: legacyVersionCodeHash,
      scriptKind: 'lock',
      cellsCount: 2,
      liveCellsCount: 1,
      capacitySum: '2000000000',
      liveCapacitySum: '1000000000',
      usedCapacitySum: '1200000000',
      liveUsedCapacitySum: '610000000',
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

function findNearestContainer(
  start: HTMLElement,
  predicate: (element: HTMLElement) => boolean
): HTMLElement {
  let current: HTMLElement | null = start;
  while (current) {
    if (predicate(current)) {
      return current;
    }
    current = current.parentElement;
  }

  throw new Error('Unable to find matching container');
}

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
    vi.mocked(api.getScript).mockResolvedValue(mockDeployments);
    vi.mocked(api.getScriptUsage).mockResolvedValue(mockUsage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getScriptCapacityChartByCodeHash).mockResolvedValue(mockCapacityChart);
    vi.mocked(api.getCell).mockImplementation(async (txHash, outputIndex) => {
      if (txHash === firstDeploymentTxHash && outputIndex === 0) {
        return {
          txHash,
          outputIndex,
          capacity: '16100000000',
          usedCapacity: 6100000000,
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
          usedCapacity: 6200000000,
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
          usedCapacity: 6000000000,
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
        liveCapacitySum: '9000000000',
        liveUsedCapacitySum: '5490000000',
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
        liveCapacitySum: '1000000000',
        liveUsedCapacitySum: '610000000',
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
        liveCapacitySum: '0',
        liveUsedCapacitySum: '0',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
      },
    });
  });

  it('renders versions first and shows deployment-bound references in version deployments', async () => {
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScript).toHaveBeenCalledWith('SECP256K1_BLAKE160');
      expect(api.getScriptUsage).toHaveBeenCalledWith('SECP256K1_BLAKE160');
      expect(screen.getByText('SECP256K1_BLAKE160')).toBeInTheDocument();
    });

    const versionsHeader = screen.getByText('Script Versions');
    const versionsPanel = findNearestContainer(versionsHeader, (element) => {
      const queries = within(element);
      return (
        queries.queryByText('Script Versions') !== null &&
        queries.queryByText('Code Hash') !== null &&
        queries.queryByText('First Deployed At') !== null &&
        queries.queryByText('Deployments') !== null &&
        queries.queryByText('Cells Using It') !== null
      );
    });
    expect(within(versionsPanel).getByText('Code Hash')).toBeInTheDocument();
    expect(within(versionsPanel).getByText('First Deployed At')).toBeInTheDocument();
    expect(within(versionsPanel).getByText('Used As')).toBeInTheDocument();
    expect(within(versionsPanel).getByText('Deployments')).toBeInTheDocument();
    expect(within(versionsPanel).getByText('Cells Using It')).toBeInTheDocument();
    expect(within(versionsPanel).getByText('Capacity Using It')).toBeInTheDocument();
    expect(within(versionsPanel).queryByText('2 versions')).toBeNull();
    expect(screen.queryByText('Script Code Cells')).toBeNull();
    expect(screen.getByRole('button', { name: 'Explain Script Versions' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Explain Script Versions' }));
    expect(screen.getByText('What this section shows')).toBeInTheDocument();
    expect(screen.getByText(/one row = one script version/i)).toBeInTheDocument();
    expect(
      within(versionsPanel).getAllByTitle(`Click to copy: ${sharedVersionCodeHash}`)
    ).toHaveLength(1);
    const sharedVersionRow = screen.getByTestId(`version-row-${sharedVersionCodeHash}`);
    expect(within(sharedVersionRow).getByText('#12,344')).toBeInTheDocument();
    expect(within(sharedVersionRow).getByText(firstDeploymentTimestampLabel)).toBeInTheDocument();
    expect(within(sharedVersionRow).getByText(/^2$/)).toBeInTheDocument();

    const deploymentsHeader = screen.getByText('Version Deployments');
    const deploymentsPanel = findNearestContainer(deploymentsHeader, (element) => {
      const queries = within(element);
      return (
        queries.queryByText('Version Deployments') !== null &&
        queries.queryByText('Outpoint') !== null &&
        queries.queryByText('Governance') !== null &&
        queries.queryByText('References') !== null &&
        queries.queryByText('Used Capacity') !== null
      );
    });
    expect(within(deploymentsPanel).getByText('Outpoint')).toBeInTheDocument();
    expect(within(deploymentsPanel).getByText('Status')).toBeInTheDocument();
    expect(within(deploymentsPanel).getByText('Governance')).toBeInTheDocument();
    expect(within(deploymentsPanel).getByText('References')).toBeInTheDocument();
    expect(within(deploymentsPanel).getByText('Deployed At')).toBeInTheDocument();
    expect(within(deploymentsPanel).getByText('Used Capacity')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Explain Version Deployments' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Explain Version Deployments' }));
    expect(screen.getAllByText('What this section shows').length).toBeGreaterThan(1);
    expect(screen.getByText(/one row = one deployment code cell/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Explain References' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Explain References' }));
    expect(screen.getByText('Reference Semantics')).toBeInTheDocument();
    expect(screen.getByText('type ref')).toBeInTheDocument();
    expect(screen.getByText(/data\/data1\/data2/i)).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: 'Reference doc: data vs type hash semantics' })
    ).toBeInTheDocument();
    expect(within(deploymentsPanel).queryByText('Type Ref')).toBeNull();
    expect(within(deploymentsPanel).queryByText('How refs work')).toBeNull();
    const deploymentsScroll = within(deploymentsPanel).getByTestId('version-deployments-scroll');
    expect(within(deploymentsScroll).getByText('Governance')).toBeInTheDocument();
    expect(
      within(deploymentsScroll).getByTitle(`Click to copy: ${firstDeploymentTxHash}:0`)
    ).toBeInTheDocument();

    const firstDeploymentRow = findNearestContainer(
      within(deploymentsPanel).getByTitle(`Click to copy: ${firstDeploymentTxHash}:0`),
      (element) => {
        const queries = within(element);
        return (
          queries.queryByTitle(`Click to copy: ${firstDeploymentTxHash}:0`) !== null &&
          queries.queryByTitle(`Click to copy: ${secondDeploymentTxHash}:1`) === null
        );
      }
    );
    expect(within(firstDeploymentRow).getByText(/^type:/i)).toBeInTheDocument();
    expect(
      within(firstDeploymentRow).getByTitle(`Click to copy: ${sharedVersionCodeHash}`)
    ).toBeInTheDocument();
    expect(
      within(firstDeploymentRow).getByTitle(`Click to copy: ${firstDeploymentDataHash}`)
    ).toBeInTheDocument();
    expect(within(firstDeploymentRow).getByText('Immutable (all-zero lock)')).toBeInTheDocument();
    expect(hasTextContent(firstDeploymentRow, '61.00000000 CKB')).toBe(true);
    expect(within(firstDeploymentRow).getByText(firstDeploymentTimestampLabel)).toBeInTheDocument();

    const secondDeploymentRow = findNearestContainer(
      within(deploymentsPanel).getByTitle(`Click to copy: ${secondDeploymentTxHash}:1`),
      (element) => {
        const queries = within(element);
        return (
          queries.queryByTitle(`Click to copy: ${secondDeploymentTxHash}:1`) !== null &&
          queries.queryByTitle(`Click to copy: ${firstDeploymentTxHash}:0`) === null
        );
      }
    );
    expect(within(secondDeploymentRow).getByText(/^type:/i)).toBeInTheDocument();
    expect(
      within(secondDeploymentRow).getByTitle(`Click to copy: ${sharedVersionCodeHash}`)
    ).toBeInTheDocument();
    expect(
      within(secondDeploymentRow).getByTitle(`Click to copy: ${secondDeploymentDataHash}`)
    ).toBeInTheDocument();
    expect(hasTextContent(secondDeploymentRow, '62.00000000 CKB')).toBe(true);
    expect(
      within(secondDeploymentRow).getByText(secondDeploymentTimestampLabel)
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(api.getCell).toHaveBeenCalledWith(firstDeploymentTxHash, 0);
      expect(api.getCell).toHaveBeenCalledWith(secondDeploymentTxHash, 1);
      expect(api.getCell).toHaveBeenCalledWith(legacyDeploymentTxHash, 0);
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: sharedVersionCodeHash,
        hashType: 'type',
        scriptKind: 'lock',
        limit: 50,
        cursor: undefined,
      });
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: firstDeploymentDataHash,
        hashType: 'data',
        scriptKind: 'lock',
        limit: 50,
        cursor: undefined,
      });
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: secondDeploymentDataHash,
        hashType: 'data',
        scriptKind: 'lock',
        limit: 50,
        cursor: undefined,
      });
      expect(
        vi
          .mocked(api.lookupScripts)
          .mock.calls.some(
            ([codeHashes]) =>
              Array.isArray(codeHashes) &&
              codeHashes.includes(secondGovernanceCodeHash) &&
              codeHashes.includes(legacyGovernanceCodeHash)
          )
      ).toBe(true);
      expect(within(secondDeploymentRow).getByText(secondGovernanceScriptName)).toBeInTheDocument();
    });
    expect(
      screen.getByText('Historical used/unused live capacity for the selected version.')
    ).toBeInTheDocument();
    expect(
      screen.getByText('Live cells currently using the selected version.')
    ).toBeInTheDocument();
    expect(
      screen.getByTitle(
        'Click to copy: 0x4444444444444444444444444444444444444444444444444444444444444444:0'
      )
    ).toBeInTheDocument();
    expect(
      screen.getByTitle(
        'Click to copy: 0x5555555555555555555555555555555555555555555555555555555555555555:1'
      )
    ).toBeInTheDocument();
    expect(
      screen.getByTitle(
        'Click to copy: 0x6666666666666666666666666666666666666666666666666666666666666666:2'
      )
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId(`version-row-${legacyVersionCodeHash}`));

    const legacyDeploymentRow = await waitFor(() =>
      findNearestContainer(
        within(deploymentsPanel).getByTitle(`Click to copy: ${legacyDeploymentTxHash}:0`),
        (element) => {
          const queries = within(element);
          return (
            queries.queryByTitle(`Click to copy: ${legacyDeploymentTxHash}:0`) !== null &&
            queries.queryByText(secondGovernanceScriptName) === null
          );
        }
      )
    );
    expect(
      within(legacyDeploymentRow).getByTitle(`Click to copy: ${legacyGovernanceCodeHash}`)
    ).toBeInTheDocument();
    expect(
      within(legacyDeploymentRow).getByTitle(`Click to copy: ${legacyGovernanceArgs}`)
    ).toBeInTheDocument();
  });

  it('keeps compact tables on tablet widths instead of switching to cards', async () => {
    setViewportWidth(900);

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScript).toHaveBeenCalledWith('SECP256K1_BLAKE160');
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
    expect(screen.getByText('Used Capacity')).toBeInTheDocument();
    expect(screen.queryByText('Status')).toBeNull();
    expect(screen.getByRole('button', { name: 'Explain Version Deployments' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Explain References' })).toBeInTheDocument();

    const firstDeploymentRow = findNearestContainer(
      screen.getByTitle(`Click to copy: ${firstDeploymentTxHash}:0`),
      (element) => {
        const queries = within(element);
        return (
          queries.queryByTitle(`Click to copy: ${firstDeploymentTxHash}:0`) !== null &&
          queries.queryByTitle(`Click to copy: ${secondDeploymentTxHash}:1`) === null
        );
      }
    );
    expect(within(firstDeploymentRow).getByText('Live')).toBeInTheDocument();
    expect(within(firstDeploymentRow).getByText('Immutable (all-zero lock)')).toBeInTheDocument();
  });

  it('switches to cards on mobile widths', async () => {
    setViewportWidth(640);

    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);

    await waitFor(() => {
      expect(api.getScript).toHaveBeenCalledWith('SECP256K1_BLAKE160');
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
    expect(within(compactDeployments).getAllByText('Used capacity').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Explain Version Deployments' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Explain References' })).toBeNull();
    expect(screen.queryByText('Status')).toBeNull();
    expect(screen.queryByText('Deployed At')).toBeNull();
    expect(screen.queryByText('Used Capacity')).toBeNull();
  });
});
