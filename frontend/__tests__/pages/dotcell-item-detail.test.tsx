import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';

import DotCellItemDetailPage from '@/app/identities/dotcell/[identityId]/client-page';
import { api } from '@/lib/api';
import { render } from '@/__tests__/utils/test-utils';

vi.mock('@/lib/api', () => ({
  api: {
    getDotCellItemDetail: vi.fn(),
    getDotCellItemActivities: vi.fn(),
    getDotCellRing: vi.fn(),
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
  useParams: () => ({ identityId: SUPPORT_ID }),
  usePathname: () => mockPathname,
  useRouter: () => ({ replace: mockReplace }),
  useSearchParams: () => mockSearchParams,
}));

/** Real testnet name: blake2b('support')[..20]. */
const SUPPORT_ID = '0x62d71147ac82b83c8531126cacb0d2f072bfd94a';
/** Real owner prefix of `support.cell`, and the full lock hash it resolves to. */
const RESOLVED_PREFIX = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867';
const RESOLVED_LOCK_HASH = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d';
const RESOLVED_ADDRESS = 'ckt1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';
/** Owner of mainnet `apt.cell`: a lock that has never appeared on chain. */
const UNRESOLVED_PREFIX = '0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019';
const MARIA_ID = '0x2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e';
const BLOG_ID = '0x3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f';
const CREATED_TX = `0x${'7'.repeat(64)}`;

const resolvedParty = {
  hashPrefix: RESOLVED_PREFIX,
  lockHash: RESOLVED_LOCK_HASH,
  address: RESOLVED_ADDRESS,
  scriptName: 'SECP256K1_BLAKE160',
};

const unresolvedParty = {
  hashPrefix: UNRESOLVED_PREFIX,
  lockHash: null,
  address: null,
  scriptName: null,
};

/** The record set testnet `maria.cell` actually carries. */
const MARIA_RECORDS = [
  {
    key: 'address.309',
    label: 'ckb',
    valueHex: '0x636b74317177',
    valueUtf8: RESOLVED_ADDRESS,
    ttl: 300,
    decodedAddress: { address: RESOLVED_ADDRESS, lockHash: RESOLVED_LOCK_HASH },
  },
  {
    key: 'address.0',
    label: 'btc',
    valueHex: '0x626331717879',
    valueUtf8: 'bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh',
    ttl: 300,
    decodedAddress: null,
  },
  {
    key: 'address.60',
    label: 'eth',
    valueHex: '0x307864384441',
    valueUtf8: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
    ttl: 300,
    decodedAddress: null,
  },
  {
    key: 'profile.email',
    label: 'work',
    valueHex: '0x6d6172696140',
    valueUtf8: 'maria@example.com',
    ttl: 300,
    decodedAddress: null,
  },
  {
    key: 'profile.phone',
    label: 'mobile',
    valueHex: '0x2b31353535',
    valueUtf8: '+15555550123',
    ttl: 300,
    decodedAddress: null,
  },
  {
    key: 'dweb.ckbfs',
    // Binary payload: no UTF-8 reading, so the hex is what gets shown.
    label: 'site',
    valueHex: '0xdeadbeef',
    valueUtf8: null,
    ttl: 300,
    decodedAddress: null,
  },
];

function buildDetail(overrides: Record<string, unknown> = {}) {
  return {
    identityId: SUPPORT_ID,
    label: 'support',
    name: 'support.cell',
    isLive: true,
    createdAtBlock: 18_000_000,
    createdAtTx: CREATED_TX,
    layoutVersion: 3,
    namespaceArgs: `0x${'5'.repeat(64)}`,
    expiredAt: 1_821_507_678,
    state: 'active',
    graceEndsAt: 1_824_099_678,
    owner: resolvedParty,
    manager: resolvedParty,
    sale: null,
    records: [],
    recordsHash: `0x${'0'.repeat(64)}`,
    nextId: MARIA_ID,
    parent: null,
    children: [],
    liveOutPoint: { txHash: CREATED_TX, index: 0 },
    ...overrides,
  };
}

describe('DotCellItemDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReplace.mockReset();
    mockSearchParams = new URLSearchParams();
    mockPathname = `/identities/dotcell/${SUPPORT_ID}`;
    vi.mocked(api.getDotCellItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
    vi.mocked(api.getDotCellRing).mockResolvedValue({
      namespaceArgs: `0x${'5'.repeat(64)}`,
      rootOutPoint: { txHash: `0x${'4'.repeat(64)}`, index: 0 },
      firstId: MARIA_ID,
      liveCount: 42,
    } as any);
  });

  it('renders the name, its state and the item endpoints it reads', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(buildDetail() as any);

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(api.getDotCellItemDetail).toHaveBeenCalledWith(SUPPORT_ID);
      expect(api.getDotCellItemActivities).toHaveBeenCalledWith(SUPPORT_ID, { limit: 50 });
    });

    expect(screen.getAllByText('support.cell').length).toBeGreaterThan(0);
    // The state shows both as the page badge and in the Name panel.
    expect(screen.getAllByText('active')).toHaveLength(2);
    expect(screen.getByRole('link', { name: /Back to \.cell Collection/ })).toHaveAttribute(
      'href',
      '/mainnet/identities/dotcell'
    );
  });

  it('links a resolved owner to its address page', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(buildDetail() as any);

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByTestId('dotcell-owner')).toBeInTheDocument();
    });
    const owner = screen.getByTestId('dotcell-owner');
    expect(within(owner).getByRole('link')).toHaveAttribute(
      'href',
      `/mainnet/address/${RESOLVED_ADDRESS}`
    );
    expect(within(owner).queryByText('unresolved')).not.toBeInTheDocument();
  });

  it('shows an unresolved owner prefix as a prefix, never as an address', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(
      buildDetail({ owner: unresolvedParty }) as any
    );

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByTestId('dotcell-owner')).toBeInTheDocument();
    });
    const owner = screen.getByTestId('dotcell-owner');
    expect(within(owner).getByText('unresolved')).toBeInTheDocument();
    expect(within(owner).queryByRole('link')).not.toBeInTheDocument();
    expect(within(owner).getByTitle(UNRESOLVED_PREFIX)).toBeInTheDocument();
    // Nothing anywhere on the page may route the bare prefix to an address.
    expect(
      screen
        .getAllByRole('link')
        .some((link) => (link.getAttribute('href') ?? '').includes(UNRESOLVED_PREFIX))
    ).toBe(false);
  });

  it('renders every record with its value and ttl', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(
      buildDetail({ records: MARIA_RECORDS }) as any
    );

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByTestId('dotcell-records')).toBeInTheDocument();
    });
    const records = screen.getByTestId('dotcell-records');
    for (const key of [
      'address.309',
      'address.0',
      'address.60',
      'profile.email',
      'profile.phone',
      'dweb.ckbfs',
    ]) {
      expect(within(records).getByText(key)).toBeInTheDocument();
    }
    expect(within(records).getAllByText('300')).toHaveLength(6);
    expect(within(records).getByText('maria@example.com')).toBeInTheDocument();
    // A binary record has no UTF-8 reading, so it shows its hex instead.
    expect(within(records).getByText('0xdeadbeef')).toBeInTheDocument();
    expect(within(records).getByRole('link', { name: RESOLVED_ADDRESS })).toHaveAttribute(
      'href',
      `/mainnet/address/${RESOLVED_ADDRESS}`
    );
  });

  it('renders the sale panel only while the name is listed', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(buildDetail() as any);
    const { unmount } = render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByTestId('dotcell-records')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('dotcell-sale')).not.toBeInTheDocument();
    unmount();

    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(
      buildDetail({
        sale: {
          priceShannons: '10000000000',
          seller: resolvedParty,
          offerOutPoint: { txHash: CREATED_TX, index: 1 },
        },
      }) as any
    );
    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByTestId('dotcell-sale')).toBeInTheDocument();
    });
    const sale = screen.getByTestId('dotcell-sale');
    expect(within(sale).getByText('100.00000000 CKB')).toBeInTheDocument();
    expect(
      within(sale)
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === `/mainnet/address/${RESOLVED_ADDRESS}`)
    ).toBe(true);
    expect(
      within(sale)
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === `/mainnet/cell/${CREATED_TX}-1`)
    ).toBe(true);
  });

  it('links the ring successor, the parent and each sub-name', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(
      buildDetail({
        parent: { identityId: MARIA_ID, label: 'maria', name: 'maria.cell' },
        children: [{ identityId: BLOG_ID, label: 'blog.maria', name: 'blog.maria.cell' }],
      }) as any
    );

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByTestId('dotcell-subnames')).toBeInTheDocument();
    });
    const subnames = screen.getByTestId('dotcell-subnames');
    expect(within(subnames).getByRole('link', { name: 'maria.cell' })).toHaveAttribute(
      'href',
      `/mainnet/identities/dotcell/${MARIA_ID}`
    );
    expect(within(subnames).getByRole('link', { name: 'blog.maria.cell' })).toHaveAttribute(
      'href',
      `/mainnet/identities/dotcell/${BLOG_ID}`
    );

    const ring = screen.getByTestId('dotcell-ring');
    expect(
      within(ring)
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === `/mainnet/identities/dotcell/${MARIA_ID}`)
    ).toBe(true);
  });

  it('stays renderable for a recycled name with no cell, sale, parent or records', async () => {
    vi.mocked(api.getDotCellItemDetail).mockResolvedValue(
      buildDetail({
        isLive: false,
        state: 'recycled',
        liveOutPoint: null,
        sale: null,
        parent: null,
        children: [],
        records: [],
      }) as any
    );

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getAllByText('recycled').length).toBeGreaterThan(0);
    });
    expect(screen.queryByTestId('dotcell-sale')).not.toBeInTheDocument();
    expect(screen.getByText('No records set.')).toBeInTheDocument();
    expect(screen.getByText('No sub-names.')).toBeInTheDocument();
  });

  it('starts a newly navigated-to name on its first activity page', async () => {
    // Parent -> sub-name navigation keeps the same route element mounted; only
    // the route parameter changes. The activity page of the name left behind
    // must not follow the user to the next one.
    const ALICE_ID = '0x4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70';
    const SHOP_ALICE_ID = '0x5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081';
    vi.mocked(api.getDotCellItemDetail).mockImplementation(async (ref: string) =>
      ref === 'alice'
        ? (buildDetail({
            identityId: ALICE_ID,
            label: 'alice',
            name: 'alice.cell',
            children: [{ identityId: SHOP_ALICE_ID, label: 'shop.alice', name: 'shop.alice.cell' }],
          }) as any)
        : (buildDetail({
            identityId: SHOP_ALICE_ID,
            label: 'shop.alice',
            name: 'shop.alice.cell',
            parent: { identityId: ALICE_ID, label: 'alice', name: 'alice.cell' },
          }) as any)
    );
    vi.mocked(api.getDotCellItemActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: true,
      nextCursor: 'alice-page-2',
    } as any);

    mockPathname = '/identities/dotcell/alice';
    const { rerender } = render(<DotCellItemDetailPage identityId="alice" />);
    await waitFor(() => {
      expect(api.getDotCellItemActivities).toHaveBeenCalledWith(ALICE_ID, { limit: 50 });
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() => {
      expect(api.getDotCellItemActivities).toHaveBeenCalledWith(ALICE_ID, {
        limit: 50,
        cursor: 'alice-page-2',
      });
    });
    expect(screen.getByText('Page 2')).toBeInTheDocument();

    // Follow the sub-name link: new path, no query string.
    mockPathname = '/identities/dotcell/shop.alice';
    mockSearchParams = new URLSearchParams();
    mockReplace.mockClear();
    vi.mocked(api.getDotCellItemActivities).mockClear();
    rerender(<DotCellItemDetailPage identityId="shop.alice" />);

    await waitFor(() => {
      expect(api.getDotCellItemActivities).toHaveBeenCalledWith(SHOP_ALICE_ID, { limit: 50 });
    });
    expect(await screen.findByText('Page 1')).toBeInTheDocument();
    expect(api.getDotCellItemActivities).not.toHaveBeenCalledWith(
      SHOP_ALICE_ID,
      expect.objectContaining({ cursor: expect.anything() })
    );
    expect(mockReplace.mock.calls.some(([url]) => String(url).includes('activity_cursor'))).toBe(
      false
    );
  });

  it('renders a not-found panel when the name is unknown', async () => {
    vi.mocked(api.getDotCellItemDetail).mockRejectedValue(new Error('API error: 404'));

    render(<DotCellItemDetailPage identityId={SUPPORT_ID} />);

    await waitFor(() => {
      expect(screen.getByText('.cell name not found')).toBeInTheDocument();
    });
  });
});
