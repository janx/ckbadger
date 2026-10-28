import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render, createTestQueryClient } from '@/__tests__/utils/test-utils';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  addressProducerCells,
  addressProducerHoldings,
} from '@/__tests__/msw/fixtures/address-producer';
import AddressDetailPage from '@/app/address/[addr]/client-page';
import fiberOpenDetail from '@/__tests__/fixtures/fiber-open-detail.json';
import {
  api,
  ApiRequestError,
  type Activity,
  type AddressCell,
  type AddressHoldings,
} from '@/lib/api';
import {
  addressFixture,
  addressHash,
  addressText,
  holdingsFixture,
  addressTokensFixture,
  addressDaoFixture,
  addressDaoCellsFixture,
  addressObjectsFixture,
  addressIdentitiesFixture,
  addressActivitiesFixture,
  addressTransactionsFixture,
  emptyComposition,
  pageFixture,
  fixtureHash,
} from '@/__tests__/msw/fixtures/address';

vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  api: {
    getAddress: vi.fn(),
    getAddressHoldings: vi.fn(),
    getAddressCells: vi.fn(),
    getAddressTokens: vi.fn(),
    getAddressDaoSummary: vi.fn(),
    getDaoDepositsByAddress: vi.fn(),
    getAddressActivities: vi.fn(),
    getAddressTransactions: vi.fn(),
    getAddressDotCellNames: vi.fn(),
    getAddressFiberChannels: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));
let route = addressText;
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ addr: route }),
  usePathname: () => `/address/${route}`,
  useRouter: () => ({ push: vi.fn() }),
}));
const daoSummary = {
  hasDaoActivity: true,
  activeDepositsCount: 5,
  pendingWithdrawalsCount: 0,
  completedWithdrawalsCount: 0,
  totalLockedCapacity: '959000000000000',
  totalLockedCkb: '9590000',
  unclaimedCompensation: '25987084255174',
  unclaimedCompensationCkb: '259870.84255174',
  totalCompensationEarned: '0',
  totalCompensationEarnedCkb: '0',
  estimatedApc: '1.99',
};
const pool = {
  lastPolledAt: new Date().toISOString(),
  enabled: true,
  healthy: true,
  count: 1,
  pendingCkbDelta: '-50000000000',
  truncated: false,
};
const poolActivity = (): Activity => ({
  ...addressActivitiesFixture[0],
  txHash: fixtureHash(900),
  blockNumber: null,
  txIndex: null,
  timestamp: null,
  poolStatus: 'proposed',
  timeAddedToPool: new Date(Date.now() - 120000).toISOString(),
  ckbDelta: '-50000000000',
  interpretation: { status: 'complete' },
});
const emptyHoldings: AddressHoldings = {
  ...holdingsFixture,
  balance: '0',
  occupiedCapacity: '0',
  freeCapacity: '0',
  daoFreeCapacity: '0',
  liveCellsCount: 0,
  depositCellsCount: 0,
  withdrawalRequestCellsCount: 0,
  holdings: { dao: 0, token: 0, object: 0, identity: 0 },
  classes: Object.fromEntries(
    Object.keys(holdingsFixture.classes).map((k) => [
      k,
      { cellCount: 0, capacity: '0', occupiedCapacity: '0' },
    ])
  ) as AddressHoldings['classes'],
  objectComposition: emptyComposition,
  objectCollections: [],
};
const namedRight = {
  identityId: '0x' + 'ef'.repeat(20),
  label: 'support',
  name: 'support.cell',
  expiredAt: 1821507678,
};
const tab = async (name: string) =>
  fireEvent.click(await screen.findByRole('button', { name: new RegExp(`^${name}`) }));
const loaded = () => screen.findByRole('heading', { name: 'Holdings' });

beforeEach(() => {
  vi.resetAllMocks();
  route = addressText;
  vi.mocked(api.getAddress).mockResolvedValue(addressFixture);
  vi.mocked(api.getAddressHoldings).mockResolvedValue(holdingsFixture);
  vi.mocked(api.getAddressTokens).mockResolvedValue(pageFixture(addressTokensFixture, 9));
  vi.mocked(api.getAddressDaoSummary).mockResolvedValue(daoSummary);
  vi.mocked(api.getDaoDepositsByAddress).mockResolvedValue(pageFixture(addressDaoFixture, 5));
  vi.mocked(api.getAddressCells).mockImplementation(async (_addr, p) =>
    pageFixture(
      p?.class === 'identity'
        ? addressIdentitiesFixture
        : p?.class === 'dao'
          ? addressDaoCellsFixture
          : addressObjectsFixture,
      p?.class === 'identity' ? 1 : 14
    )
  );
  vi.mocked(api.getAddressActivities).mockResolvedValue(pageFixture(addressActivitiesFixture));
  vi.mocked(api.getAddressTransactions).mockResolvedValue(
    pageFixture(addressTransactionsFixture, 1021)
  );
  vi.mocked(api.getAddressDotCellNames).mockResolvedValue(pageFixture([]));
  vi.mocked(api.getAddressFiberChannels).mockResolvedValue([]);
});

describe('Address v3', () => {
  it('renders the real address Fiber endpoint empty array without a page crash', async () => {
    vi.mocked(api.getAddressFiberChannels).mockResolvedValue([]);
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.queryByRole('heading', { name: 'Fiber channels' })).not.toBeInTheDocument();
  });

  it('links a producer-backed channel from the real address Fiber array response', async () => {
    vi.mocked(api.getAddressFiberChannels).mockResolvedValue([fiberOpenDetail] as never);
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByRole('heading', { name: 'Fiber channels' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /0xf0f0f0/i })).toHaveAttribute(
      'href',
      `/mainnet/fiber/channels/${fiberOpenDetail.channelId}`
    );
    expect(screen.getByText('open')).toBeInTheDocument();
    expect(api.getAddressFiberChannels).toHaveBeenCalledWith(addressHash, { limit: 100 });
  });

  it('shows typed 404 evidence only when the address endpoint actually returns 404', async () => {
    vi.mocked(api.getAddress).mockRejectedValue(new ApiRequestError(404, 'not_found', 'absent'));
    render(<AddressDetailPage />);
    expect(await screen.findByText(`/addresses/${addressText}`)).toBeInTheDocument();
    expect(
      screen.getByText('This explorer could not open the address at this address.')
    ).toBeInTheDocument();
  });

  it('keeps an invalid address 400 as a read error', async () => {
    vi.mocked(api.getAddress).mockRejectedValue(
      new ApiRequestError(400, 'bad_request', 'invalid address')
    );
    render(<AddressDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid address');
    expect(
      screen.queryByText('This explorer could not open the address at this address.')
    ).not.toBeInTheDocument();
  });
  it('uses one holdings snapshot for balance and the exact three-way partition', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      balance: '1',
      commonKnowledgeSize: '0',
      liveCellsCount: 1,
    });
    const { container } = render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByLabelText('Balance')).toHaveTextContent('9,651,178.35338057');
    expect(screen.getByLabelText('Address capacity partition')).toHaveTextContent(
      '54,571.35338057'
    );
    expect(screen.getByLabelText('Address capacity partition')).toHaveTextContent(
      '9,589,480.00000000'
    );
    expect(container.querySelector('[data-tier="2"]')).toHaveClass('mt-[72px]');
  });
  it('shows all four holdings classes with exact distinct counts', async () => {
    render(<AddressDetailPage />);
    await loaded();
    const nav = screen.getByRole('navigation', { name: 'Holdings' });
    for (const name of ['Nervos DAO 5', 'Tokens 9', 'Objects 14', 'Identities 1'])
      expect(within(nav).getByRole('link', { name: new RegExp(name) })).toBeInTheDocument();
    expect(screen.getByText('29 holdings, one mark each')).toBeInTheDocument();
    expect(await screen.findByRole('link', { name: 'valar.bit' })).toHaveAttribute(
      'href',
      expect.stringContaining('/identities/bit-cell/')
    );
  });
  it('keeps full object totals and composition separate from one page of items', async () => {
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) =>
      p?.class === 'identity'
        ? pageFixture(addressIdentitiesFixture)
        : p?.class === 'dao'
          ? pageFixture(addressDaoCellsFixture)
          : {
              ...pageFixture(addressObjectsFixture.slice(0, 2)),
              hasMore: true,
              nextCursor: 'objects-next',
            }
    );
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByText('11 of 14')).toBeInTheDocument();
    expect(await screen.findByText('Showing 1-2 of 14 objects')).toBeInTheDocument();
    expect(screen.getAllByText('BTC+CKB').length).toBeGreaterThan(0);
  });
  it('links object tiles and exact collection identity', async () => {
    render(<AddressDetailPage />);
    await loaded();
    expect(
      await screen.findByRole('link', { name: `Object ${addressObjectsFixture[0].display!.id}` })
    ).toHaveAttribute(
      'href',
      expect.stringContaining(`/objects/${addressObjectsFixture[0].display!.id}`)
    );
    expect(screen.getByRole('link', { name: '拯救民主' })).toHaveAttribute(
      'href',
      expect.stringContaining(`/clusters/${fixtureHash(200)}`)
    );
  });
  it('retains exact token digits and unknown decimals raw markers', async () => {
    vi.mocked(api.getAddressTokens).mockResolvedValue(
      pageFixture([
        {
          ...addressTokensFixture[0],
          balance: '9007199254740993123456789',
          decimals: null,
          name: null,
          symbol: null,
        },
      ])
    );
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText(/9,007,199,254,740,993,123,456,789/)).toHaveTextContent('raw');
    expect(screen.getByRole('link', { name: /0x00000000.*012c/ })).toHaveAttribute(
      'href',
      expect.stringContaining('/tokens/')
    );
  });
  it('pages tokens beyond 100 without treating page length as holdings count', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue({
      ...holdingsFixture,
      holdings: { ...holdingsFixture.holdings, token: 101 },
    });
    vi.mocked(api.getAddressTokens).mockImplementation(async (_a, p) =>
      p?.cursor
        ? pageFixture([{ ...addressTokensFixture[0], name: 'Last token' }])
        : { ...pageFixture(addressTokensFixture), hasMore: true, nextCursor: 'next-token' }
    );
    render(<AddressDetailPage />);
    await screen.findByText('Showing 1-9 of 101 token kinds');
    const section = screen.getByRole('heading', { name: /Tokens/ }).closest('section')!;
    fireEvent.click(within(section).getByRole('button', { name: 'Next' }));
    expect(await screen.findByText('Last token')).toBeInTheDocument();
    expect(api.getAddressTokens).toHaveBeenLastCalledWith(addressHash, {
      limit: 50,
      cursor: 'next-token',
    });
  });
  it('links occupancy legend to the same holdings group on hover and focus', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    const legend = screen.getByRole('button', { name: /Objects 3,761/ });
    fireEvent.mouseEnter(legend);
    expect(container.querySelector('[data-holding-class="object"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-space-class="object"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    fireEvent.mouseLeave(legend);
    expect(container.querySelector('[data-holding-class="object"]')).not.toHaveAttribute(
      'data-highlighted'
    );
    fireEvent.focus(legend);
    expect(container.querySelector('[data-space-class="object"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
  });
  it('preserves the registered lock name and deprecated state', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      lockScriptInfo: { ...addressFixture.lockScriptInfo!, deprecated: true },
    });
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByRole('link', { name: 'JoyID' })).toHaveAttribute(
      'href',
      expect.stringContaining('/scripts/JoyID')
    );
    expect(screen.getByText('Deprecated')).toBeInTheDocument();
  });
  it('shows raw script identity when no registered name is present', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      address: undefined,
      lockScriptInfo: undefined,
    });
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByRole('heading', { name: 'Address' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Lock script' })).toBeInTheDocument();
    expect(screen.queryByText('Active')).not.toBeInTheDocument();
  });
  it('does not invent zero totals when holdings cannot be read', async () => {
    vi.mocked(api.getAddressHoldings).mockRejectedValue(new Error('owner index broken'));
    render(<AddressDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('owner index broken');
    expect(screen.queryByText('0 holdings, one mark each')).not.toBeInTheDocument();
  });
  it('shows real zero holdings without suppressing named-only .cell rights and history', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue(emptyHoldings);
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      balance: '0',
      commonKnowledgeSize: '0',
      liveCellsCount: 0,
    });
    vi.mocked(api.getAddressDotCellNames).mockResolvedValue(pageFixture([namedRight]));
    vi.mocked(api.getAddressActivities).mockResolvedValue(
      pageFixture([
        { ...addressActivitiesFixture[0], ckbDelta: '0', usedDelta: '0', roles: ['owner'] },
      ])
    );
    vi.mocked(api.getAddressTransactions).mockResolvedValue(
      pageFixture([{ ...addressTransactionsFixture[0], txType: 'named', capacityChange: '0' }])
    );
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByText('0 holdings, one mark each')).toBeInTheDocument();
    expect(await screen.findByRole('link', { name: 'support.cell' })).toHaveAttribute(
      'href',
      expect.stringContaining('/identities/dotcell/')
    );
    expect(screen.getByText(/These rights do not add/)).toBeInTheDocument();
    expect(await screen.findByText('Roles: owner')).toBeInTheDocument();
    await tab('Transactions');
    expect((await screen.findAllByText('Named participant')).length).toBeGreaterThan(0);
  });
  it('pages named rights without using one page as the total', async () => {
    vi.mocked(api.getAddressDotCellNames).mockImplementation(async (_a, p) =>
      p?.cursor
        ? pageFixture([{ ...namedRight, name: 'next.cell' }])
        : { ...pageFixture([namedRight]), hasMore: true, nextCursor: 'next-name' }
    );
    render(<AddressDetailPage />);
    const section = await screen.findByTestId('address-dotcell-names');
    expect(within(section).getByText('Showing 1-1 names')).toBeInTheDocument();
    fireEvent.click(within(section).getByRole('button', { name: 'Next' }));
    expect(await screen.findByText('next.cell')).toBeInTheDocument();
    expect(api.getAddressDotCellNames).toHaveBeenLastCalledWith(addressHash, {
      limit: 50,
      cursor: 'next-name',
    });
  });
  it('omits named rights when none are present', async () => {
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.queryByTestId('address-dotcell-names')).not.toBeInTheDocument();
  });
  it('requests live-cell class filtering before pagination and resets its cursor', async () => {
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) => ({
      ...pageFixture(
        p?.class === 'identity'
          ? addressIdentitiesFixture
          : p?.class === 'dao'
            ? addressDaoCellsFixture
            : addressObjectsFixture
      ),
      hasMore: p?.class !== 'identity',
      nextCursor: p?.class === 'identity' ? null : 'next-cell',
    }));
    render(<AddressDetailPage />);
    await loaded();
    await tab('Live cells');
    const history = screen.getByRole('region', { name: 'History' });
    fireEvent.click(await within(history).findByRole('button', { name: 'Next' }));
    await waitFor(() =>
      expect(api.getAddressCells).toHaveBeenCalledWith(addressHash, {
        limit: 50,
        cursor: 'next-cell',
        class: undefined,
      })
    );
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Filter live cells' })).getByRole('button', {
        name: /Identities/,
      })
    );
    await waitFor(() =>
      expect(api.getAddressCells).toHaveBeenLastCalledWith(addressHash, {
        limit: 50,
        cursor: undefined,
        class: 'identity',
      })
    );
    expect(await within(history).findAllByText(/valar.bit/)).not.toHaveLength(0);
  });
  it('never uses transaction count as the activities count', async () => {
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByRole('button', { name: 'Activities' })).toHaveTextContent(/^Activities$/);
  });
  it('uses separate type and lock script-call filters and resets activity cursor', async () => {
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.filter === 'all'
        ? { ...pageFixture(addressActivitiesFixture), hasMore: true, nextCursor: 'next-act' }
        : pageFixture([])
    );
    render(<AddressDetailPage />);
    await loaded();
    const history = screen.getByRole('region', { name: 'History' });
    fireEvent.click(await within(history).findByRole('button', { name: 'Next' }));
    await waitFor(() =>
      expect(api.getAddressActivities).toHaveBeenLastCalledWith(addressHash, {
        limit: 50,
        cursor: 'next-act',
        filter: 'all',
      })
    );
    fireEvent.click(screen.getByRole('button', { name: 'Script calls (type)' }));
    expect(
      await screen.findByText('No Script calls (type) activities on this page.')
    ).toBeInTheDocument();
    expect(api.getAddressActivities).toHaveBeenLastCalledWith(addressHash, {
      limit: 50,
      cursor: undefined,
      filter: 'type_call',
    });
    fireEvent.click(screen.getByRole('button', { name: 'Script calls (lock)' }));
    expect(
      await screen.findByText('No Script calls (lock) activities on this page.')
    ).toBeInTheDocument();
  });
  it('resets filters, tab and cursors when the route changes', async () => {
    const { rerender } = render(<AddressDetailPage />);
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Token' }));
    await tab('Transactions');
    route = 'ckb1other';
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      lockScriptHash: fixtureHash(999),
    });
    rerender(<AddressDetailPage />);
    await waitFor(() =>
      expect(api.getAddressActivities).toHaveBeenCalledWith(fixtureHash(999), {
        limit: 50,
        cursor: undefined,
        filter: 'all',
      })
    );
    expect(screen.getByRole('button', { name: 'Activities' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
  });
  it.each(['dotbit', 'did_ckb', 'bit_cell', 'dotcell'] as const)(
    'links %s identity deltas through the shared activity row',
    async (standard) => {
      vi.mocked(api.getAddressActivities).mockResolvedValue(
        pageFixture([
          {
            ...addressActivitiesFixture[0],
            itemDeltas: [{ kind: 'identity', standard, identityId: fixtureHash(77), delta: 1 }],
          },
        ])
      );
      render(<AddressDetailPage />);
      await loaded();
      await waitFor(() =>
        expect(
          screen
            .getAllByRole('link')
            .some((l) =>
              l
                .getAttribute('href')
                ?.includes(
                  standard === 'did_ckb'
                    ? '/identities/did/'
                    : standard === 'bit_cell'
                      ? '/identities/bit-cell/'
                      : `/identities/${standard}/`
                )
            )
        ).toBe(true)
      );
    }
  );
  it('describes only this participant movement without claiming transaction mint or burn', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue(
      pageFixture([
        {
          ...addressActivitiesFixture[0],
          tags: 1,
          itemDeltas: [
            { kind: 'token', typeScriptHash: fixtureHash(300), delta: '1', decimals: 8 },
          ],
        },
      ])
    );
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText('Token received')).toBeInTheDocument();
    expect(screen.queryByText('Token mint')).not.toBeInTheDocument();
  });
  it('renders token changes and script evidence alongside exact CKB position', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue(
      pageFixture([
        {
          ...addressActivitiesFixture[0],
          itemDeltas: [
            { kind: 'token', typeScriptHash: fixtureHash(300), delta: '123456789', decimals: 8 },
          ],
          typeCalls: [
            {
              typeCodeHash: fixtureHash(1),
              typeHashType: 'type',
              typeArgs: '0x',
              scriptHash: fixtureHash(2),
              scriptName: 'xUDT',
            },
          ],
        },
      ])
    );
    render(<AddressDetailPage />);
    await loaded();
    expect((await screen.findAllByText(/1.23456789/)).length).toBeGreaterThan(0);
    expect(
      screen.getAllByRole('link').some((l) => l.getAttribute('href')?.includes('/scripts/xUDT'))
    ).toBe(true);
  });
  it('keeps unconfirmed summary apart from the canonical balance across tabs', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      pendingSummary: { txCount: 2, capacityDelta: '-50000000000' },
    });
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByLabelText('Unconfirmed')).toHaveTextContent('2 tx');
    expect(screen.getByLabelText('Balance')).toHaveTextContent('9,651,178.35338057');
    await tab('Live cells');
    expect(screen.getByLabelText('Unconfirmed')).toBeInTheDocument();
  });
  it('does not print a pending amount for exact zero net change', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      pendingSummary: { txCount: 1, capacityDelta: '0' },
    });
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.getByLabelText('Unconfirmed')).not.toHaveTextContent('CKB');
  });
  it('does not invent an unconfirmed summary when absent', async () => {
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.queryByLabelText('Unconfirmed')).not.toBeInTheDocument();
  });
  it('shows pool status and partial interpretation without a block link', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      ...pageFixture([
        {
          ...poolActivity(),
          interpretation: {
            status: 'partial',
            reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
          },
        },
      ]),
      pool,
    });
    render(<AddressDetailPage />);
    await loaded();
    const history = screen.getByRole('region', { name: 'History' });
    expect(await within(history).findByText('Proposed')).toBeInTheDocument();
    expect(within(history).getByText(/partial: input not yet resolvable/)).toBeInTheDocument();
    expect(
      within(history)
        .queryAllByRole('link')
        .some((l) => l.getAttribute('href')?.includes('/blocks/'))
    ).toBe(false);
  });
  it('counts only committed activity rows in the page range', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      ...pageFixture([poolActivity(), ...addressActivitiesFixture]),
      pool,
    });
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText('Showing 1-1 activities')).toBeInTheDocument();
  });
  it.each([
    ['truncated', true, true, 'More unconfirmed transactions than shown.'],
    ['unavailable', true, false, 'Pool view unavailable'],
    ['disabled', false, false, null],
  ] as const)('preserves the %s pool notice', async (_name, enabled, healthy, text) => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      ...pageFixture([]),
      pool: { ...pool, enabled, healthy, truncated: true },
    });
    render(<AddressDetailPage />);
    await loaded();
    if (text) expect(await screen.findByText(new RegExp(text))).toBeInTheDocument();
    else expect(screen.queryByText(/Pool view unavailable/)).not.toBeInTheDocument();
  });
  it('keeps pool summary on page two and across activity filters', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      pendingSummary: { txCount: 2, capacityDelta: '1' },
    });
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      ...pageFixture(addressActivitiesFixture),
      hasMore: true,
      nextCursor: 'next',
    });
    render(<AddressDetailPage />);
    await loaded();
    fireEvent.click(
      await within(screen.getByRole('region', { name: 'History' })).findByRole('button', {
        name: 'Next',
      })
    );
    expect(screen.getByLabelText('Unconfirmed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'DAO' }));
    expect(screen.getByLabelText('Unconfirmed')).toBeInTheDocument();
  });
  it('gives compact and desktop transactions the same pool and partial evidence', async () => {
    vi.mocked(api.getAddressTransactions).mockResolvedValue({
      ...pageFixture([
        {
          ...addressTransactionsFixture[0],
          blockNumber: null,
          timestamp: null,
          poolStatus: 'proposed',
          timeAddedToPool: new Date(Date.now() - 120000).toISOString(),
          interpretation: {
            status: 'partial',
            reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
          },
        },
      ]),
      pool,
    });
    render(<AddressDetailPage />);
    await loaded();
    await tab('Transactions');
    const mobile = await screen.findByTestId('address-tx-row-mobile');
    expect(within(mobile).getByText('Proposed')).toBeInTheDocument();
    expect(within(mobile).getByText(/in pool for 2m/)).toBeInTheDocument();
    expect(within(mobile).getByText(/partial: input not yet resolvable/)).toBeInTheDocument();
  });
  it('prints transaction amount, fee, byte size and cycle status without loss', async () => {
    vi.mocked(api.getAddressTransactions).mockResolvedValue(
      pageFixture([
        {
          ...addressTransactionsFixture[0],
          capacityChange: '-9007199254740993',
          cycles: null,
          cyclesStatus: 'failed',
        },
      ])
    );
    render(<AddressDetailPage />);
    await loaded();
    await tab('Transactions');
    const table = await screen.findByRole('table', { name: 'Address transactions' });
    expect(table).toHaveTextContent('90,071,992.54740993');
    expect(table).toHaveTextContent('0.00012430');
    expect(table).toHaveTextContent('5,597');
    expect(within(table).getByText('Failed')).toBeInTheDocument();
    expect(table.querySelector('a[href*="/tx/"] span')).not.toHaveClass('text-gold');
  });
  it('keeps nullable identity metadata and decode pending explicit', async () => {
    const rows: AddressCell[] = [
      {
        ...addressIdentitiesFixture[0],
        expiresAt: null,
        display: { ...addressIdentitiesFixture[0].display!, name: null },
      },
    ];
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) =>
      pageFixture(
        p?.class === 'identity'
          ? rows
          : p?.class === 'dao'
            ? addressDaoCellsFixture
            : addressObjectsFixture
      )
    );
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText(/Expiry unavailable/)).toBeInTheDocument();
    expect(screen.getByText(/held since/)).toBeInTheDocument();
  });
  it.each([
    ['holdings', 'address-holdings', 'getAddressHoldings'],
    ['tokens', 'address-tokens', 'getAddressTokens'],
    ['objects', 'address-holdings-cells', 'getAddressCells'],
    ['activities', 'address-activities', 'getAddressActivities'],
  ] as const)('reports stale %s when a background refresh fails', async (label, key, method) => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <AddressDetailPage />
      </QueryClientProvider>
    );
    await loaded();
    await waitFor(() =>
      expect(
        client
          .getQueryCache()
          .findAll({ queryKey: [key] })
          .some((q) => q.state.status === 'success')
      ).toBe(true)
    );
    vi.mocked(api[method]).mockRejectedValue(new Error('refresh failed'));
    await act(async () => {
      await client.invalidateQueries({ queryKey: [key] });
    });
    expect(
      (await screen.findAllByRole('alert')).some((el) =>
        el.textContent?.includes('Showing previously loaded')
      )
    ).toBe(true);
    expect(screen.getByLabelText('Balance')).toHaveTextContent('9,651,178.35338057');
  });
  it('names every identity standard with its canonical label', async () => {
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText('.bit Cell')).toBeInTheDocument();
  });
  it('consumes real producer JSON including transferred DAO, null metadata and exact script references', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue(addressProducerHoldings);
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) => ({
      ...addressProducerCells,
      data: addressProducerCells.data.filter((c) => !p?.class || c.class === p.class),
    }));
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText('1 deposits · 1 withdrawal requests')).toBeInTheDocument();
    const request = addressProducerCells.data.find((c) => c.dao?.status === 'withdrawing')!;
    expect(request.dao!.lockScriptHash).not.toBe(request.lockScriptHash);
    expect(screen.getByRole('link', { name: 'withdraw requested' })).toHaveAttribute(
      'href',
      expect.stringContaining(`/cell/${request.txHash}-${request.outputIndex}`)
    );
    expect(screen.getByRole('link', { name: 'Current mNFT group' })).toHaveAttribute(
      'href',
      expect.stringContaining('/classes/')
    );
    await tab('Live cells');
    const table = await screen.findByRole('table', { name: 'Live cells' });
    const script = addressProducerCells.data.find((c) => c.class === 'script')!;
    expect(table.querySelector(`a[href*="${script.typeCodeHash}"]`)).toHaveAttribute(
      'href',
      expect.stringContaining('hashType=data1&kind=type')
    );
    expect(table).toHaveTextContent('1,000.00000000');
  });
  it('dates a transferred DAO withdrawal request at its request block, not the original deposit', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue(addressProducerHoldings);
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) => ({
      ...addressProducerCells,
      data: addressProducerCells.data.filter((c) => !p?.class || c.class === p.class),
    }));
    render(<AddressDetailPage />);
    await loaded();
    const requestLink = await screen.findByRole('link', { name: 'withdraw requested' });
    const row = requestLink.parentElement!;
    expect(within(row).getByRole('link', { name: '#7' })).toHaveAttribute(
      'href',
      expect.stringContaining('/blocks/7')
    );
    expect(within(row).queryByRole('link', { name: '#5' })).not.toBeInTheDocument();
  });
  it('copies the complete address with an accessible action', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    render(<AddressDetailPage />);
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Copy address' }));
    expect(await screen.findByRole('button', { name: 'Address copied' })).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith(addressText);
  });
  it('retains all exact object fixture sums in the complete board scope', () => {
    expect(
      addressObjectsFixture.reduce((s, c) => s + BigInt(c.capacity), BigInt(0)).toString()
    ).toBe(holdingsFixture.classes.object.capacity);
    expect(
      addressObjectsFixture.reduce((s, c) => s + BigInt(c.occupiedCapacity), BigInt(0)).toString()
    ).toBe(holdingsFixture.classes.object.occupiedCapacity);
  });
});
