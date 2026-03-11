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
      usedCapacity: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
    vi.mocked(api.getMnftItemActivities).mockResolvedValue({
      data: [],
      limit: 20,
      hasMore: false,
      nextCursor: null,
    } as any);
  });

  it('renders mnft detail sections', async () => {
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
      expect(screen.getByText('Asset Snapshot')).toBeInTheDocument();
    });

    expect(screen.getByText('Identity Graph')).toBeInTheDocument();
    expect(screen.getByText('On-chain State')).toBeInTheDocument();
    expect(screen.getByText('Ownership & Live Cell')).toBeInTheDocument();
    expect(screen.getByText('Class Context')).toBeInTheDocument();
    expect(screen.getByText('Lifecycle')).toBeInTheDocument();
    expect(screen.getByText('Activities')).toBeInTheDocument();
    expect(screen.getByText('Class A')).toBeInTheDocument();
    expect(screen.getByText('Issuer A')).toBeInTheDocument();
    expect(screen.getByText('Class A #99')).toBeInTheDocument();
  });

  it('renders not found panel when item is missing', async () => {
    vi.mocked(api.getMnftItemDetail).mockRejectedValue(new Error('API error: 404'));

    render(<MnftItemDetailPage objectId="0xmnft" />);

    await waitFor(() => {
      expect(screen.getByText('mNFT item not found')).toBeInTheDocument();
    });
  });
});
