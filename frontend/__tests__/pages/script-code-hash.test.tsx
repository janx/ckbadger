import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    lookupScripts: vi.fn(),
    getCodeCell: vi.fn(),
    getScriptOccupationChartByCodeHash: vi.fn(),
    getCellsByScriptRef: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockCodeHash = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';

vi.mock('next/navigation', () => ({
  useParams: () => ({ codeHash: mockCodeHash }),
  useSearchParams: () => new URLSearchParams(),
}));

const emptyCells = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptByCodeHashPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
        codeHash: mockCodeHash,
        name: 'Spore Cluster',
        scriptKind: 'type',
        decoderType: null,
        hashType: 'type',
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 15,
        liveCapacitySum: '25000000000',
        liveOccupiedCapacitySum: '14000000000',
      },
    });
    vi.mocked(api.getCodeCell).mockResolvedValue({ txHash: null, outputIndex: null });
    vi.mocked(api.getScriptOccupationChartByCodeHash).mockResolvedValue({
      title: 'Occupation',
      series: [
        { key: 'occupied', label: 'Occupied', color: '#f59e0b' },
        { key: 'unoccupied', label: 'Unoccupied', color: '#00c389' },
      ],
      data: [
        {
          date: '2024-01-15',
          values: { occupied: '1000000000', unoccupied: '1500000000' },
        },
      ],
    });
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(emptyCells);
  });

  it('renders capacity utilization for the script code hash', async () => {
    render(<ScriptByCodeHashPage />);

    await waitFor(() => {
      expect(screen.getByText('Capacity Utilization')).toBeInTheDocument();
      expect(screen.getByText('Occupation History')).toBeInTheDocument();
    });

    expect(screen.getByText(/^Occupied:/)).toBeInTheDocument();
    expect(screen.getByText(/^Unoccupied:/)).toBeInTheDocument();
  });
});
