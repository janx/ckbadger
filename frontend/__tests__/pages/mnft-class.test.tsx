import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import MnftClassDetailPage from '@/app/classes/[classId]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getObjectCollection: vi.fn(),
    getObjectCollectionCapacityChart: vi.fn(),
    getObjectCollectionItems: vi.fn(),
    getObjectCollectionHolders: vi.fn(),
    getObjectCollectionActivities: vi.fn(),
    getAddress: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockClassId = '0xabcdef1234567890abcdef1234567890abcdef1234567890';
const mockReplace = vi.fn();

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ classId: mockClassId }),
  useRouter: () => ({ push: vi.fn(), replace: mockReplace }),
  usePathname: () => `/classes/${mockClassId}`,
  useSearchParams: () => new URLSearchParams(''),
}));

const mockCollection = {
  collectionId: mockClassId,
  standard: 'm-nft',
  name: 'Test mNFT Class',
  totalCount: 100,
  liveCount: 80,
  holdersCount: 25,
  activitiesCount: 50,
  ownedCapacity: '500000000000',
  ownedKnowledge: '300000000000',
  storageProfile: {
    tier: 'unknown' as const,
    fullyOnchainCount: 0,
    fullyOnCkbCount: 0,
    decentralizedDependentCount: 0,
    centralizedDependentCount: 0,
    unknownCount: 80,
    fullyOnchainRatio: '0',
  },
  classDetail: {
    classId: mockClassId,
    issuerId: '0x1111111111111111111111111111111111111111',
    name: 'Test mNFT Class',
    description: 'A test mNFT collection',
    renderer: null,
    total: 100,
    issued: 100,
    configure: 3,
  },
  issuerDetail: {
    issuerId: '0x1111111111111111111111111111111111111111',
    name: 'Test Issuer',
    classCount: 5,
    setCount: 2,
    infoHex: null,
  },
  createdAtBlock: 500000,
  ownerLockHash: '0x2222222222222222222222222222222222222222222222222222222222222222',
};

describe('MnftClassDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getObjectCollectionCapacityChart).mockResolvedValue({
      title: 'mNFT Capacity History',
      data: [],
      series: [],
    });
    vi.mocked(api.getObjectCollectionActivities).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getAddress).mockResolvedValue({
      lockScriptHash: mockCollection.ownerLockHash,
      address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
      transactionsCount: 0,
    } as any);
  });

  it('renders loading state', () => {
    vi.mocked(api.getObjectCollection).mockReturnValue(new Promise(() => {}));
    render(<MnftClassDetailPage classId={mockClassId} />);
    expect(screen.getByTestId('header')).toBeInTheDocument();
  });

  it('renders not found state', async () => {
    vi.mocked(api.getObjectCollection).mockRejectedValue(new Error('404 Not Found'));
    render(<MnftClassDetailPage classId={mockClassId} />);
    await waitFor(() => {
      expect(screen.getByText('mNFT Class not found')).toBeInTheDocument();
    });
  });

  it('renders collection overview with class metadata', async () => {
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);
    render(<MnftClassDetailPage classId={mockClassId} />);
    await waitFor(() => {
      expect(screen.getByText('Test mNFT Class')).toBeInTheDocument();
    });
    expect(screen.getByText('mNFT Class')).toBeInTheDocument();
    expect(screen.getByText('100')).toBeInTheDocument(); // supply
    expect(screen.getByText('25')).toBeInTheDocument(); // holders
  });

  it('renders class context section with issuer info', async () => {
    vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);
    render(<MnftClassDetailPage classId={mockClassId} />);
    await waitFor(() => {
      expect(screen.getByText('Class Context')).toBeInTheDocument();
    });
    expect(screen.getByText('Test Issuer')).toBeInTheDocument();
    expect(screen.getByText('transferable, burnable')).toBeInTheDocument();
  });

  it('renders collection overview without class metadata', async () => {
    const collectionNoClass = {
      ...mockCollection,
      classDetail: undefined,
      issuerDetail: undefined,
    };
    vi.mocked(api.getObjectCollection).mockResolvedValue(collectionNoClass);
    render(<MnftClassDetailPage classId={mockClassId} />);
    await waitFor(() => {
      expect(screen.getByText('Test mNFT Class')).toBeInTheDocument();
    });
    expect(screen.queryByText('Class Context')).not.toBeInTheDocument();
  });
});
