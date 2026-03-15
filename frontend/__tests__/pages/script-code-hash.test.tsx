import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    lookupScripts: vi.fn(),
    getCodeCell: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCellsByScriptRef: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockCodeHash = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';
const mockDataHash = '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649';
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
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
        codeHash: mockCodeHash,
        name: 'Unknown',
        scriptKind: 'type',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: mockCodeHash,
        deploymentDataHash: mockDataHash,
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 15,
        liveCapacitySum: '25000000000',
        liveUsedCapacitySum: '14000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
      },
    });
    vi.mocked(api.getCodeCell).mockResolvedValue({ txHash: null, outputIndex: null });
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

  it('renders deployment refs and queries cells for the script code hash', async () => {
    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    await waitFor(() => {
      expect(api.getScriptCapacityChartByCodeHash).toHaveBeenCalledWith(mockCodeHash, 'both');
      expect(api.getCellsByScriptRef).toHaveBeenCalledWith({
        codeHash: mockCodeHash,
        hashType: 'type',
        scriptKind: 'both',
        limit: 50,
        cursor: undefined,
      });
      expect(screen.getByText('Capacity History')).toBeInTheDocument();
      expect(screen.getByText('Cells Capacity')).toBeInTheDocument();
      expect(screen.getByText(/HMul:/)).toBeInTheDocument();
    });
    expect(screen.getByText('Same Deployment References')).toBeInTheDocument();
    expect(screen.getByText('Reference Semantics')).toBeInTheDocument();
    expect(screen.getByText(/bytecode hash ref family \(data\/data1\/data2\)/)).toBeInTheDocument();
    expect(screen.getByText(/Tradeoff: choose type for upgradeability/)).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: 'Reference doc: data vs type hash semantics' })
    ).toHaveAttribute('href', 'https://docs.nervos.org/docs/tech-explanation/data-type-diff');
    expect(screen.getByText('type (upgradeable ref)')).toBeInTheDocument();
    expect(screen.getByText('Type + Data')).toBeInTheDocument();
    expect(
      screen
        .getAllByRole('link')
        .some(
          (link) => link.getAttribute('href') === `/script/${mockCodeHash}?hashType=type&kind=type`
        )
    ).toBe(true);
    expect(
      screen
        .getAllByRole('link')
        .some(
          (link) => link.getAttribute('href') === `/script/${mockDataHash}?hashType=data&kind=type`
        )
    ).toBe(true);
  });

  it('redirects known script hash to the unified named script detail page', async () => {
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
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
        liveCapacitySum: '25000000000',
        liveUsedCapacitySum: '14000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
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
