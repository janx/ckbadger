import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    lookupScripts: vi.fn(),
    getCodeCell: vi.fn(),
    getCodeCells: vi.fn(),
    getCell: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCellsByScriptRef: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockCodeHash = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';
const mockDataHash = '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649';
const mockGovernanceCodeHash = '0xf222222222222222222222222222222222222222222222222222222222222222';
const mockGovernanceArgs = '0xa222222222222222222222222222222222222222';
const mockDeploymentTxHash = '0x4444444444444444444444444444444444444444444444444444444444444444';
const { replaceMock } = vi.hoisted(() => ({ replaceMock: vi.fn() }));
let currentCodeHashParam = mockCodeHash;

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ codeHash: currentCodeHashParam }),
  useSearchParams: () => new URLSearchParams(),
  useRouter: () => ({ replace: replaceMock }),
}));

const emptyCells = {
  data: [],
  total: 0,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptByCodeHashPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    currentCodeHashParam = mockCodeHash;
    vi.mocked(api.lookupScripts).mockImplementation(async (codeHashes) => {
      const response: Record<string, any> = {};

      if (codeHashes.includes(mockCodeHash)) {
        response[mockCodeHash] = {
          referenceHash: mockCodeHash,
          codeHash: mockCodeHash,
          name: 'Unknown',
          scriptKind: 'type',
          decoderType: null,
          hashType: 'type',
          deploymentTypeHash: mockCodeHash,
          deploymentDataHash: mockDataHash,
          codeCellTxHash: mockDeploymentTxHash,
          codeCellOutputIndex: 0,
          liveCellsCount: 15,
          ownedCapacitySum: '25000000000',
          ownedKnowledgeSum: '14000000000',
          codeCellsLiveCount: 1,
          codeCellsTotal: 1,
          resolutionState: 'resolved',
          ambiguity: null,
        };
      }

      if (codeHashes.includes(mockGovernanceCodeHash)) {
        response[mockGovernanceCodeHash] = {
          referenceHash: mockGovernanceCodeHash,
          codeHash: mockGovernanceCodeHash,
          name: 'Governance Lock',
          scriptKind: 'lock',
          decoderType: null,
          hashType: 'type',
          deploymentTypeHash: mockGovernanceCodeHash,
          deploymentDataHash: null,
          codeCellTxHash: null,
          codeCellOutputIndex: null,
          liveCellsCount: 0,
          ownedCapacitySum: '0',
          ownedKnowledgeSum: '0',
          codeCellsLiveCount: 0,
          codeCellsTotal: 0,
          resolutionState: 'resolved',
          ambiguity: null,
        };
      }

      return response;
    });
    vi.mocked(api.getCodeCell).mockResolvedValue({ txHash: null, outputIndex: null });
    vi.mocked(api.getCodeCells).mockResolvedValue({
      codeCells: [
        {
          txHash: mockDeploymentTxHash,
          outputIndex: 0,
          status: 'live',
          createdAtBlock: 123456,
          capacity: '16100000000',
        },
      ],
      liveCount: 1,
      totalCount: 1,
      resolvedVersionHash: mockCodeHash,
      ambiguity: null,
    });
    vi.mocked(api.getCell).mockResolvedValue({
      txHash: mockDeploymentTxHash,
      outputIndex: 0,
      capacity: '16100000000',
      commonKnowledgeSize: 6100000000,
      lockScriptHash: '0xlock1',
      dataSize: 0,
      createdAtBlock: 123456,
      status: 'live',
      lock: {
        codeHash: mockGovernanceCodeHash,
        hashType: 'type',
        args: mockGovernanceArgs,
      },
    });
    vi.mocked(api.getScriptCapacityChartByCodeHash).mockResolvedValue({
      title: 'Capacity History',
      series: [
        { key: 'used', label: 'Used', color: '#f59e0b' },
        { key: 'unused', label: 'Unused', color: '#00c389' },
      ],
      data: [
        {
          date: '2024-01-15',
          values: { used: '1000000000', unused: '1500000000' },
        },
      ],
    });
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(emptyCells);
  });

  it('renders unknown code hash with the unified script detail layout', async () => {
    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    await waitFor(() => {
      expect(screen.getByText('Script Versions')).toBeInTheDocument();
      expect(screen.getByText('Version Deployments')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Explain Script Versions' })).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Explain Version Deployments' })
      ).toBeInTheDocument();
    });
    expect(screen.queryByText('Same Deployment References')).toBeNull();
    expect(screen.getByText('Usage')).toBeInTheDocument();
    expect(
      screen.getByText(
        'Historical common knowledge and free live capacity for the selected version.'
      )
    ).toBeInTheDocument();
    expect(screen.getAllByTitle(`Click to copy: ${mockCodeHash}`).length).toBeGreaterThan(0);
    expect(screen.getByTitle(`Click to copy: ${mockDeploymentTxHash}:0`)).toBeInTheDocument();
  });

  it('redirects known script hash to the unified named script detail page', async () => {
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
        referenceHash: mockCodeHash,
        codeHash: mockCodeHash,
        name: 'Default Lock',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: mockCodeHash,
        deploymentDataHash: mockDataHash,
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 15,
        ownedCapacitySum: '25000000000',
        ownedKnowledgeSum: '14000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
        resolutionState: 'resolved',
        ambiguity: null,
      },
    });

    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    await waitFor(() => {
      expect(replaceMock).toHaveBeenCalledWith(
        `/scripts/${encodeURIComponent('Default Lock')}?ref=${mockCodeHash}&hashType=type&kind=lock`
      );
    });
  });

  it('redirects /script/<name> alias to the named detail page', async () => {
    currentCodeHashParam = 'Default Lock';

    render(<ScriptByCodeHashPage codeHash="Default Lock" />);

    await waitFor(() => {
      expect(replaceMock).toHaveBeenCalledWith(`/scripts/${encodeURIComponent('Default Lock')}`);
    });
  });
});
