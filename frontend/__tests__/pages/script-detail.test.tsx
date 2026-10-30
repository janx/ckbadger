import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
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
    getCodeCells: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isNotFoundApiError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'status' in error && error.status === 404,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: 'SECP256K1_BLAKE160' }),
  usePathname: () => '/scripts/SECP256K1_BLAKE160',
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
  // /scripts/{name} always carries the family's registered references, classes and history.
  references: [
    {
      referenceHash: sharedVersionCodeHash,
      hashType: 'type',
      liveCellsCount: 7,
      cellsCount: 8,
      ownedCapacitySum: '9000000000',
      ownedKnowledgeSum: '5490000000',
      currentVersionHash: sharedVersionCodeHash,
    },
    {
      referenceHash: sharedObservedData1Reference,
      hashType: 'data1',
      liveCellsCount: 1,
      cellsCount: 2,
      ownedCapacitySum: '1000000000',
      ownedKnowledgeSum: '610000000',
      currentVersionHash: sharedVersionCodeHash,
    },
  ],
  codeClasses: [],
  history: [],
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
  totalPeak: { date: '2024-01-15', value: '10000000000' },
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
        commonKnowledgeSize: '6100000000',
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
        commonKnowledgeSize: '7700000000',
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
        commonKnowledgeSize: '9300000000',
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
          commonKnowledgeSize: '6100000000',
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
          commonKnowledgeSize: '6200000000',
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
          commonKnowledgeSize: '6000000000',
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

  it('follows the board order: History, code versions, cells; references in the developer tier', async () => {
    setViewportWidth(900);
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    expect(await screen.findByRole('heading', { name: 'SECP256K1_BLAKE160' })).toBeInTheDocument();
    expect(api.getScriptFamilyDetail).toHaveBeenCalledWith('SECP256K1_BLAKE160');
    const tier1 = document.querySelector('[data-tier="1"]')!;
    const tier2 = document.querySelector('[data-tier="2"]')!;
    expect(
      within(tier1 as HTMLElement).getByRole('heading', { name: 'History' })
    ).toBeInTheDocument();
    expect(
      within(tier1 as HTMLElement).getByRole('heading', { name: 'Code versions' })
    ).toBeInTheDocument();
    expect(
      within(tier1 as HTMLElement).getByRole('heading', { name: 'Its cells' })
    ).toBeInTheDocument();
    expect(
      within(tier2 as HTMLElement).getByRole('heading', { name: 'Registered references' })
    ).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Code versions' })).toBeInTheDocument();
    expect(screen.queryByRole('table', { name: 'Code cell deployments' })).toBeNull();
    expect(screen.queryByTestId('script-versions-panel')).toBeNull();
  });

  it('provides compact cell and version records at mobile widths', async () => {
    setViewportWidth(390);
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    expect(await screen.findByRole('list', { name: 'Code versions, compact' })).toBeInTheDocument();
    expect(await screen.findByRole('list', { name: 'Its cells, compact' })).toBeInTheDocument();
    expect(document.querySelector('[data-tier="2"]')).toHaveClass('mt-[72px]');
  });

  it('wraps the complete current binary in the compact registered reference row', async () => {
    setViewportWidth(390);
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue({
      ...mockScriptFamilyDetail,
      references: [
        {
          referenceHash: sharedObservedData1Reference,
          hashType: 'type',
          currentVersionHash: sharedVersionCodeHash,
        },
      ],
    } as any);
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const compact = await screen.findByRole('list', { name: 'Registered references, compact' });
    const current = within(compact).getByText(`Current code: ${sharedVersionCodeHash}`);
    expect(current).toHaveClass('break-all');
    expect(current).toHaveTextContent(sharedVersionCodeHash);
  });

  it.each([
    { status: 500, expectedTitle: 'Script data unavailable' },
    { status: 404, expectedTitle: 'An unanswered name.' },
  ])(
    'distinguishes named script HTTP $status from not found',
    async ({ status, expectedTitle }) => {
      vi.mocked(api.getScriptFamilyDetail).mockRejectedValue({
        status,
        message: `family endpoint ${status}`,
      });
      render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
      expect(await screen.findByRole('heading', { name: expectedTitle })).toBeInTheDocument();
      if (status !== 404) {
        expect(screen.getByRole('alert')).toHaveTextContent('family endpoint 500');
      } else {
        expect((await screen.findAllByText('/scripts/SECP256K1_BLAKE160')).length).toBeGreaterThan(
          0
        );
      }
    }
  );

  it.each(['lookup', 'code cells'])(
    'shows a raw reference %s request failure instead of not found',
    async (failedRequest) => {
      vi.mocked(api.getCodeCells).mockResolvedValue({ deployments: [] } as any);
      if (failedRequest === 'lookup') {
        vi.mocked(api.lookupScripts).mockRejectedValue(new Error('lookup transport failed'));
      } else {
        vi.mocked(api.getCodeCells).mockRejectedValue(new Error('code cells invariant failed'));
      }
      render(<ScriptDetailPage codeHash={sharedVersionCodeHash} />);
      expect(
        await screen.findByRole('heading', { name: 'Script data unavailable' })
      ).toBeInTheDocument();
      expect(screen.getByRole('alert')).toHaveTextContent(
        failedRequest === 'lookup' ? 'lookup transport failed' : 'code cells invariant failed'
      );
      expect(screen.queryByText('No script matched this name or reference.')).toBeNull();
    }
  );

  it('keeps a resolved raw reference visible as an error when its code-cell query returns 404', async () => {
    vi.mocked(api.getCodeCells).mockRejectedValue({ status: 404, message: 'reference missing' });
    render(<ScriptDetailPage codeHash={sharedVersionCodeHash} />);
    expect(
      await screen.findByRole('heading', { name: 'Script data unavailable' })
    ).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('reference missing');
    expect(
      screen.queryByText('This explorer could not open the script at this address.')
    ).not.toBeInTheDocument();
  });

  it('marks deprecated family and versions without calling them unlabeled', async () => {
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue({
      ...mockScriptFamilyDetail,
      name: 'PW Lock',
      description: 'Ethereum wallet compatible lock',
      versions: mockScriptFamilyDetail.versions.map((version) => ({
        ...version,
        name: 'PW Lock',
        deprecated: true,
      })),
    } as any);
    vi.mocked(api.getScriptUsage).mockResolvedValue({ ...mockUsage, name: 'PW Lock' });
    render(<ScriptDetailPage name="PW Lock" />);
    expect(await screen.findByRole('heading', { name: 'PW Lock' })).toBeInTheDocument();
    expect(screen.getByText(/Deprecated/)).toBeInTheDocument();
    expect(screen.queryByText('Unlabeled Script')).toBeNull();
  });

  it('keeps a version without a code-cell deployment visible', async () => {
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
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const table = await screen.findByRole('table', { name: 'Code versions' });
    const row = within(table).getAllByRole('row')[1];
    expect(row).toHaveTextContent('v1');
    expect(row).toHaveTextContent(sharedVersionCodeHash);
    expect(row).toHaveTextContent('No deployment observed');
  });

  it('shows failed version usage as unavailable, never zero', async () => {
    vi.mocked(api.getScriptUsage).mockRejectedValue(new Error('usage endpoint 503'));
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const table = await screen.findByRole('table', { name: 'Code versions' });
    const alerts = within(table).getAllByRole('alert');
    expect(alerts.length).toBeGreaterThan(0);
    expect(alerts[0]).toHaveAttribute('data-state', 'failed');
    expect(alerts[0]).toHaveAttribute(
      'title',
      'The usage of SECP256K1_BLAKE160 failed to load: usage endpoint 503'
    );
    expect(alerts[0]).not.toHaveTextContent(/^0$/);
  });

  it('does not infer zero when one version is absent from usage', async () => {
    vi.mocked(api.getScriptUsage).mockResolvedValue({
      ...mockUsage,
      byDeployment: mockUsage.byDeployment.filter((d) => d.codeHash !== legacyVersionCodeHash),
    });
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const table = await screen.findByRole('table', { name: 'Code versions' });
    await waitFor(() =>
      expect(
        within(table)
          .getAllByTestId('version-usage')
          .some((node) => node.getAttribute('data-state') === 'missing')
      ).toBe(true)
    );
    expect(
      within(table)
        .getAllByTestId('version-usage')
        .some((node) => node.textContent === '7')
    ).toBe(true);
  });

  it('lists the cells of every registered reference against the family total (SC-S10)', async () => {
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const cells = await screen.findByRole('table', { name: 'Its cells' });
    expect(within(cells).getByText('Owner')).toBeInTheDocument();
    expect(within(cells).getByText('Also guarded by')).toBeInTheDocument();
    expect(within(cells).getByText('Occupied')).toBeInTheDocument();
    await waitFor(() => expect(document.body.textContent).toMatch(/of 8\s*cells/));
    for (const [codeHash, hashType] of [
      [sharedVersionCodeHash, 'type'],
      [sharedObservedData1Reference, 'data1'],
    ])
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith(
        expect.objectContaining({ codeHash, hashType, scriptKind: 'lock' })
      );
  });

  it('says so when no live cell uses the script', async () => {
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    expect(await screen.findByText('No live cells use SECP256K1_BLAKE160')).toBeInTheDocument();
  });

  it('retains failed selected usage as an error in version rows', async () => {
    vi.mocked(api.getScriptUsage).mockRejectedValue(new Error('usage endpoint 503'));
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const table = await screen.findByRole('table', { name: 'Code versions' });
    expect(within(table).getAllByTestId('version-usage')[0]).toHaveAttribute(
      'data-state',
      'failed'
    );
    expect(document.body.textContent).not.toMatch(/of 0\s*cells/);
  });

  it('retains missing selected usage as missing in version rows', async () => {
    vi.mocked(api.getScriptUsage).mockResolvedValue({
      ...mockUsage,
      byDeployment: mockUsage.byDeployment.filter((d) => d.codeHash !== sharedVersionCodeHash),
    });
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    const table = await screen.findByRole('table', { name: 'Code versions' });
    await waitFor(() =>
      expect(
        within(table)
          .getAllByTestId('version-usage')
          .some((node) => node.getAttribute('data-state') === 'missing')
      ).toBe(true)
    );
  });

  it('shows missing virtual occupied size of a genesis burn cell as an error', async () => {
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue({
      data: [
        {
          txHash: `0x${'77'.repeat(32)}`,
          outputIndex: 0,
          capacity: '840000000000000000',
          commonKnowledgeSize: '0',
          lockScriptHash: '0xburn',
          dataSize: 0,
          createdAtBlock: 0,
          cellType: 'genesis_special_burn',
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    render(<ScriptDetailPage name="SECP256K1_BLAKE160" />);
    expect((await screen.findAllByTestId('burn-cell-virtual-size-error')).length).toBeGreaterThan(
      0
    );
    expect(screen.queryByTitle(/Virtual common knowledge size: 0/)).toBeNull();
  });

  it('reads the genesis burn virtual size from the API value', async () => {
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue({
      data: [
        {
          txHash: `0x${'77'.repeat(32)}`,
          outputIndex: 0,
          capacity: '840000000000000000',
          commonKnowledgeSize: '0',
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
    expect(
      (
        await screen.findAllByTitle(
          `Virtual common knowledge size: ${formatCapacity('504000000000000000')}`
        )
      ).length
    ).toBeGreaterThan(0);
    expect(screen.queryByTitle(/5\.04B/)).toBeNull();
  });
});
