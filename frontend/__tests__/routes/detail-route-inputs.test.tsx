import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    lookupScripts: vi.fn(),
    getCodeCell: vi.fn(),
    getCodeCells: vi.fn(),
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

vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useSearchParams: () => new URLSearchParams(),
  useRouter: () => ({ replace: vi.fn() }),
}));

describe('detail route inputs', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
        referenceHash: mockCodeHash,
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
        ownedCapacitySum: '25000000000',
        ownedKnowledgeSum: '14000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
        resolutionState: 'resolved',
        ambiguity: null,
      },
    });
    vi.mocked(api.getCodeCell).mockResolvedValue({ txHash: null, outputIndex: null });
    vi.mocked(api.getCodeCells).mockResolvedValue({
      codeCells: [],
      liveCount: 0,
      totalCount: 0,
      resolvedVersionHash: mockCodeHash,
      ambiguity: null,
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
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue({
      data: [],
      total: 0,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });
  });

  it('renders script detail from explicit route props without next params', async () => {
    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    await waitFor(() => {
      expect(
        screen.getByText(
          'Historical common knowledge and free live capacity for the selected version.'
        )
      ).toBeInTheDocument();
    });
  });
});
