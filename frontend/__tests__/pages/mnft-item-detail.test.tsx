import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';

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
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockReplace = vi.fn();
let mockSearchParams = new URLSearchParams();
let mockPathname = '';

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ objectId: '0xmnft' }),
  usePathname: () => mockPathname,
  useRouter: () => ({ replace: mockReplace }),
  useSearchParams: () => mockSearchParams,
}));

describe('MnftItemDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockReset();
    mockSearchParams = new URLSearchParams();
    mockPathname = '/objects/mnft/0xmnft';
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
      '/mainnet/inventory/objects'
    );
    // Properties panel: state & configure
    expect(screen.getByText('locked')).toBeInTheDocument();
    expect(screen.getByText('transferable, burnable')).toBeInTheDocument();

    // Issuer & Class panel
    expect(screen.getAllByText('Class A').length).toBeGreaterThan(0);
    expect(screen.getByText('Issuer A')).toBeInTheDocument();

    // Links: created at block, cell outpoint, class
    expect(
      screen
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === '/mainnet/blocks/123')
    ).toBe(true);
    expect(
      screen
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === '/mainnet/cell/0xtx-4')
    ).toBe(true);
    expect(
      screen
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === '/mainnet/classes/0xclass')
    ).toBe(true);

    // Payload Data hex viewer
    expect(screen.getByText(/Payload Data/)).toBeInTheDocument();
    expect(screen.getByText('8 bytes', { exact: false })).toBeInTheDocument();
  });

  it('starts a newly navigated-to token on its first activity page', async () => {
    // Moving from one token to another on the same route (search, history)
    // changes only the route parameter, so the element stays mounted. The
    // activity page of the token left behind must not follow the user.
    const detailFor = (nftId: string, tokenIndex: number) => ({
      nftId,
      standard: 'm-nft',
      isLive: true,
      ownerLockHash: '0xlock',
      createdAtBlock: 123,
      tokenIndex,
      characteristicHex: '0x',
      configure: 0,
      state: 0,
      txHash: '0xtx',
      outputIndex: 0,
      class: {
        classId: '0xclass',
        issuerId: '0xissuer',
        name: 'Class A',
        description: null,
        renderer: null,
        total: 1000,
        issued: 200,
        configure: 0,
      },
      issuer: {
        issuerId: '0xissuer',
        name: 'Issuer A',
        classCount: 2,
        setCount: 3,
        infoHex: '0x7b7d',
      },
      lifecycle: [],
    });
    vi.mocked(api.getMnftItemDetail).mockImplementation(
      async (id: string) => (id === '0xaaa' ? detailFor('0xaaa', 1) : detailFor('0xbbb', 2)) as any
    );
    vi.mocked(api.getMnftItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: true,
      nextCursor: 'token-1-page-2',
    } as any);

    mockPathname = '/objects/mnft/0xaaa';
    const { rerender } = render(<MnftItemDetailPage objectId="0xaaa" />);
    await waitFor(() => {
      expect(api.getMnftItemActivities).toHaveBeenCalledWith('0xaaa', { limit: 50 });
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() => {
      expect(api.getMnftItemActivities).toHaveBeenCalledWith('0xaaa', {
        limit: 50,
        cursor: 'token-1-page-2',
      });
    });
    expect(screen.getByText('Page 2')).toBeInTheDocument();

    mockPathname = '/objects/mnft/0xbbb';
    mockSearchParams = new URLSearchParams();
    mockReplace.mockClear();
    vi.mocked(api.getMnftItemActivities).mockClear();
    rerender(<MnftItemDetailPage objectId="0xbbb" />);

    await waitFor(() => {
      expect(api.getMnftItemActivities).toHaveBeenCalledWith('0xbbb', { limit: 50 });
    });
    expect(await screen.findByText('Page 1')).toBeInTheDocument();
    expect(api.getMnftItemActivities).not.toHaveBeenCalledWith(
      '0xbbb',
      expect.objectContaining({ cursor: expect.anything() })
    );
    expect(mockReplace.mock.calls.some(([url]) => String(url).includes('activity_cursor'))).toBe(
      false
    );
  });

  it('renders not found panel when item is missing', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(new Error('API error: 404'));

    render(<MnftItemDetailPage objectId="0xmnft" />);

    await waitFor(() => {
      expect(screen.getByText('mNFT item not found')).toBeInTheDocument();
    });
  });
});
