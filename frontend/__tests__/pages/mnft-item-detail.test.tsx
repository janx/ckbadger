import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';

import MnftItemDetailPage from '@/app/objects/mnft/[objectId]/client-page';
import { api } from '@/lib/api';
import { render } from '../utils/test-utils';

vi.mock('@/lib/api', () => ({
  api: {
    getMnftItemDetail: vi.fn(),
    getAddress: vi.fn(),
    getMnftItemActivities: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockReplace = vi.fn();
let mockSearchParams = new URLSearchParams();

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ objectId: '0xmnft' }),
  usePathname: () => '/objects/mnft/0xmnft',
  useRouter: () => ({ replace: mockReplace }),
  useSearchParams: () => mockSearchParams,
}));

describe('MnftItemDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockReset();
    mockSearchParams = new URLSearchParams();
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: '0xlock',
      address: 'ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3',
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getMnftItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders mnft identity, ownership, and lifecycle links', async () => {
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      nftId: '0xmnft',
      standard: 'm-nft',
      isLive: true,
      ownerLockHash: '0xlock',
      createdAtBlock: 123,
      tokenIndex: 99,
      characteristicHex: '0x0102030405060708',
      configure: 3,
      state: 1,
      txHash: '0xtx',
      outputIndex: 4,
      class: {
        classId: '0xclass',
        issuerId: '0xissuer',
        name: 'Class A',
        description: 'Class description',
        renderer: 'renderer:v1',
        total: 1000,
        issued: 200,
        configure: 1,
      },
      issuer: {
        issuerId: '0xissuer',
        name: 'Issuer A',
        classCount: 2,
        setCount: 3,
        infoHex: '0x7b7d',
      },
      lifecycle: [
        {
          event: 'mint',
          blockNumber: 123,
          txHash: null,
          outputIndex: null,
          note: 'minted',
        },
        {
          event: 'live',
          blockNumber: null,
          txHash: '0xtx',
          outputIndex: 4,
          note: 'live',
        },
      ],
    });

    render(<MnftItemDetailPage objectId="0xmnft" />);

    await waitFor(() => {
      expect(screen.getByText('Class A #99')).toBeInTheDocument();
    });

    // Breadcrumb navigation
    const breadcrumb = screen.getByRole('navigation');
    expect(breadcrumb).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /^Objects$/ })).toHaveAttribute(
      'href',
      '/inventory/objects'
    );
    expect(screen.getByText('M-NFT')).toBeInTheDocument();
    expect(screen.getByText('locked')).toBeInTheDocument();
    expect(screen.getByText('transferable, burnable')).toBeInTheDocument();
    expect(screen.getAllByText('Class A').length).toBeGreaterThan(0);
    expect(screen.getByText('Issuer A')).toBeInTheDocument();
    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/blocks/123')
    ).toBe(true);
    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/cell/0xtx-4')
    ).toBe(true);
    expect(
      screen.getAllByRole('link').some((link) => link.getAttribute('href') === '/classes/0xclass')
    ).toBe(true);
  });

  it('renders not found panel when item is missing', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(new Error('API error: 404'));

    render(<MnftItemDetailPage objectId="0xmnft" />);

    await waitFor(() => {
      expect(screen.getByText('mNFT item not found')).toBeInTheDocument();
    });
  });
});
