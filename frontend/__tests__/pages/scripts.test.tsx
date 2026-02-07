import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScripts: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockScripts = {
  data: [
    {
      codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
      name: 'Default Lock',
      description: 'SECP256K1/blake160 is the default lock script.',
      scriptKind: 'lock',
      rfc: 'https://github.com/nervosnetwork/rfcs/...',
      website: null,
      sourceUrl: 'https://github.com/nervosnetwork/ckb-system-scripts/...',
      decoderType: null,
      network: 'mainnet',
      hashType: 'type',
      dataHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
      typeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
      tag: null,
      deprecated: false,
      isSystem: true,
      codeCellTxHash: null,
      codeCellOutputIndex: null,
    },
    {
      codeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
      name: 'Nervos DAO',
      description: 'Nervos DAO is a smart contract.',
      scriptKind: 'type',
      rfc: 'https://github.com/nervosnetwork/rfcs/...',
      website: null,
      sourceUrl: 'https://github.com/nervosnetwork/ckb-system-scripts/...',
      decoderType: 'dao',
      network: 'mainnet',
      hashType: 'type',
      dataHash: '0x32064a14ce10d95d4b7343054cc19d73b25b16ae61a6c681011ca781a60c7923',
      typeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
      tag: null,
      deprecated: false,
      isSystem: true,
      codeCellTxHash: null,
      codeCellOutputIndex: null,
    },
  ],
  total: 61,
  limit: 20,
  hasMore: true,
  nextCursor: 'Nervos DAO',
};

describe('ScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders scripts list', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('Default Lock')).toBeInTheDocument();
    });

    expect(screen.getByText('Nervos DAO')).toBeInTheDocument();
  });

  it('shows loading state initially', async () => {
    vi.mocked(api.getScripts).mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve(mockScripts), 100))
    );

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('Scripts')).toBeInTheDocument();
    });
  });

  it('displays script kind badges', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('lock')).toBeInTheDocument();
      expect(screen.getByText('type')).toBeInTheDocument();
    });
  });

  it('displays system badge for system scripts', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      const systemBadges = screen.getAllByText('System');
      expect(systemBadges.length).toBeGreaterThan(0);
    });
  });

  it('shows empty state when no scripts found', async () => {
    vi.mocked(api.getScripts).mockResolvedValue({
      data: [],
      total: 0,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('No scripts found')).toBeInTheDocument();
    });
  });

  it('displays total count in footer', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText(/Total: 61 scripts/)).toBeInTheDocument();
    });
  });

  it('displays deprecated badge for deprecated scripts', async () => {
    const deprecatedScripts = {
      data: [
        {
          ...mockScripts.data[0],
          deprecated: true,
          isSystem: false,
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    };
    vi.mocked(api.getScripts).mockResolvedValue(deprecatedScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('Deprecated')).toBeInTheDocument();
    });
  });

  it('has search functionality', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      const searchInput = screen.getByPlaceholderText('Search by name or code hash...');
      expect(searchInput).toBeInTheDocument();
    });
  });

  it('has pagination controls', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScripts);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(screen.getByText('Previous')).toBeInTheDocument();
      expect(screen.getByText('Next')).toBeInTheDocument();
    });
  });
});
