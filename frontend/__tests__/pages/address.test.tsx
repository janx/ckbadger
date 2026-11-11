import { describe, it, expect, vi, beforeEach, onTestFinished } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { render, createTestQueryClient } from '@/__tests__/utils/test-utils';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import {
  expectBefore,
  expectNamePlate,
  expectOnePageSeal,
  expectOneRowSeal,
} from '@/__tests__/utils/seals';
import { addressCellSealId } from '@/lib/address-cell-seal';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { PromptHeading } from '@/components/ui/prompt-heading';
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
// Phase 5 (A21): the holdings heading runs on the address's live cells.
const loaded = () => screen.findByRole('heading', { name: /^Holdings [\d,]+ cells?$/ });

beforeEach(() => {
  vi.resetAllMocks();
  route = addressText;
  vi.mocked(api.getAddress).mockResolvedValue(addressFixture);
  vi.mocked(api.getAddressHoldings).mockResolvedValue(holdingsFixture);
  vi.mocked(api.getAddressTokens).mockResolvedValue(pageFixture(addressTokensFixture, 9));
  vi.mocked(api.getAddressDaoSummary).mockResolvedValue(daoSummary);
  vi.mocked(api.getDaoDepositsByAddress).mockResolvedValue(pageFixture(addressDaoFixture, 5));
  vi.mocked(api.getAddressCells).mockImplementation(async (_addr, p) =>
    // C14 (A6): a collection page holds that collection's objects only, as the API filters.
    p?.collection
      ? pageFixture(
          addressObjectsFixture.filter((c) => c.display!.collectionId === p.collection),
          holdingsFixture.objectCollections.find((g) => g.id === p.collection)!.itemCount
        )
      : pageFixture(
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
    expect(screen.queryByRole('heading', { name: /^Fiber channels/ })).not.toBeInTheDocument();
    // Consistency plan C1: the tab names the page.
    expect(document.title).toMatch(/^ck[bt]1[0-9a-z]{4}…[0-9a-z]{6} · address · CKBadger$/);
  });

  it('links a producer-backed channel from the real address Fiber array response', async () => {
    vi.mocked(api.getAddressFiberChannels).mockResolvedValue([fiberOpenDetail] as never);
    render(<AddressDetailPage />);
    await loaded();
    // phase 5 (D4): the command runs on the channels listed
    expect(await screen.findByRole('heading', { name: 'Fiber channels 1' })).toBeInTheDocument();
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
    expect(screen.getByText('no address at this address.')).toBeInTheDocument();
  });

  // Visual finish V8: the page loads as the shared detail skeleton, not a "Loading…" line.
  it('shows the detail skeleton while the address loads, then the page', async () => {
    let resolve!: (value: typeof addressFixture) => void;
    vi.mocked(api.getAddress).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      })
    );
    render(<AddressDetailPage />);
    const skeleton = screen.getByRole('status', { name: 'Loading …' });
    expect(skeleton.children).toHaveLength(3);
    expect(screen.queryByText('Loading address…')).toBeNull();
    resolve(addressFixture);
    await loaded();
    expect(screen.queryByRole('status', { name: 'Loading …' })).toBeNull();
  });

  // The seals by kind (C4): an address is its lock script, so its page presses the script's
  // stone (the tall rounded one, 104 px, 80 below md, −4°) of the lock script hash the address
  // read already carries; nothing else is fetched for it.
  it('presses the lock script’s stone beside the address, from the hash the address read carries', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    expect(addressFixture.lockScriptHash).toBe(addressHash);
    const seal = expectOnePageSeal(container, { kind: 'script', hash: addressHash });
    expect(seal).toHaveAttribute('data-seal-form', 'page');
    // the old hash seal is gone from the head
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
    const head = seal.closest('[data-sealed-head]')!;
    expect(head).toContainElement(screen.getByRole('heading', { level: 1 }));
    expectBefore(seal, screen.getByRole('heading', { level: 1 }));
    expect(container.querySelector('[data-tier="1"]')).not.toContainElement(seal);
  });

  // The name plate (seals by kind, C10): beside the lock script's stone, the kind line "Address"
  // with the page's status (the lock, the live cells and transactions), the short address, then the
  // full address and its copy button; nothing under them (the address page has no lede).
  it('stands the address’s name plate beside the stone: Address and its status, the short address, the full one', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    const { stone, kindLine, nameBox, title, below } = expectNamePlate(container, 'Address');
    expect(stone).toHaveAttribute('data-seal', 'script');
    expect(below).toBeNull();
    expect(kindLine.children).toHaveLength(2);
    expect(kindLine).toHaveTextContent(/^Addresslock JoyID40 live cells · 1,021 transactions$/);
    expect(within(kindLine).getByRole('link', { name: 'JoyID' })).toHaveAttribute(
      'href',
      expect.stringContaining('/scripts/JoyID')
    );
    expect(title).toHaveTextContent(
      new RegExp(`^${addressText.slice(0, 12)}…${addressText.slice(-8)}$`)
    );
    expect(title).toHaveClass('text-entity', 'max-md:text-index');
    expect(nameBox.children).toHaveLength(2);
    const idLine = nameBox.lastElementChild as HTMLElement;
    expect(idLine).toHaveTextContent(new RegExp(`^${addressText}$`));
    expect(idLine).toContainElement(screen.getByRole('button', { name: 'Copy address' }));
  });

  // The seals by kind (C3 for the address's cells): the Live cells tab leads every row with the
  // cell's own row seal, classed as the transaction page classes the same cell; the page's one
  // page seal stays the address's.
  it('leads every live cell with the cell’s own seal, and the page keeps one page seal, the lock’s', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue(addressProducerHoldings);
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) => ({
      ...addressProducerCells,
      data: addressProducerCells.data.filter((c) => !p?.class || c.class === p.class),
    }));
    const { container } = render(<AddressDetailPage />);
    await loaded();
    await tab('Live cells');
    const table = await screen.findByRole('table', { name: 'Live cells' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(addressProducerCells.data.length);
    rows.forEach((row, i) => {
      const cell = addressProducerCells.data[i];
      const seal = expectOneRowSeal(row, addressCellSealId(cell));
      expect(row.querySelector('td')).toContainElement(seal);
      expectBefore(
        seal,
        within(row).getByRole('link', { name: new RegExp(`:${cell.outputIndex}$`) })
      );
    });
    expectOnePageSeal(container, { kind: 'script', hash: addressHash });
  });

  it('keeps an invalid address 400 as a read error', async () => {
    vi.mocked(api.getAddress).mockRejectedValue(
      new ApiRequestError(400, 'bad_request', 'invalid address')
    );
    render(<AddressDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid address');
    expect(screen.queryByText('no address at this address.')).not.toBeInTheDocument();
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
    expect(container.querySelector('[data-tier="2"]')).toHaveClass(
      'mt-24',
      'border-t',
      'border-base-border'
    );
  });
  it('shows all four holdings classes with exact distinct counts', async () => {
    render(<AddressDetailPage />);
    await loaded();
    // C14: the classes it holds are one line of links by the heading (the 29-mark grid is gone).
    const nav = screen.getByRole('navigation', { name: 'Holdings' });
    expect(
      within(nav)
        .getAllByRole('link')
        .map((l) => l.textContent)
    ).toEqual(['5 DAO cells', '9 tokens', '14 objects', '1 identity']);
    // Phase 5 (A21): the holdings run on the live cells; the count nav stands beside the heading,
    // outside its name, and the section is named by the heading.
    const heading = screen.getByRole('heading', { name: 'Holdings 40 cells' });
    expectPrompt(heading, 'Holdings', '40 cells');
    expect(heading.parentElement).toContainElement(nav);
    expect(heading).not.toContainElement(nav);
    expect(screen.getByRole('region', { name: 'Holdings 40 cells' })).toContainElement(heading);
    expect(screen.queryByText(/holdings, one mark each/)).toBeNull();
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
    // C8: the composition block counts every object; an opened collection pages its own.
    expect(screen.getByRole('group', { name: 'Object composition' })).toHaveTextContent(
      '11 of 14 fully on-chain'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open 拯救民主' }));
    expect(await screen.findByText('Showing 1–2 of 6 objects · 10 per page')).toBeInTheDocument();
    expect(screen.getAllByText('BTC+CKB').length).toBeGreaterThan(0);
  });
  it('links object tiles and exact collection identity', async () => {
    render(<AddressDetailPage />);
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Open 拯救民主' }));
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
  it('names an xUDT-compatible token standard, not its raw enum (BA-30)', async () => {
    vi.mocked(api.getAddressTokens).mockResolvedValue(
      pageFixture([{ ...addressTokensFixture[0], standard: 'xudt_compatible' }])
    );
    render(<AddressDetailPage />);
    await loaded();
    expect(await screen.findByText('xUDT-compatible')).toBeInTheDocument();
    expect(screen.queryByText('xudt_compatible')).not.toBeInTheDocument();
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
    await screen.findByText('Showing 1–9 of 101 token kinds · 20 per page');
    const section = screen.getByRole('heading', { name: /Tokens/ }).closest('section')!;
    fireEvent.click(within(section).getByRole('button', { name: 'Next' }));
    expect(await screen.findByText('Last token')).toBeInTheDocument();
    expect(api.getAddressTokens).toHaveBeenLastCalledWith(addressHash, {
      limit: 20,
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
    // Lane P: the name heads the page's status line and the Lock metric, both to the script.
    const links = screen.getAllByRole('link', { name: 'JoyID' });
    expect(links).toHaveLength(2);
    for (const link of links)
      expect(link).toHaveAttribute('href', expect.stringContaining('/scripts/JoyID'));
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
    // C13: without an address the page is named by its short lock hash.
    expect(
      screen.getByRole('heading', {
        level: 1,
        name: `${addressHash.slice(0, 12)}…${addressHash.slice(-8)}`,
      })
    ).toBeInTheDocument();
    // Lane E (C6): the Address board's tier-2 heading (was "Lock script").
    expect(
      screen.getByRole('heading', { name: 'The lock script this address names' })
    ).toBeInTheDocument();
    // with no address, the derived names are the lock hash alone
    expect(
      within(screen.getByRole('region', { name: 'Derived names' })).getAllByRole('list')
    ).toHaveLength(1);
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
    // C14: classes it does not hold are one line by the heading, not four empty sections.
    const none = screen.getByText('no DAO deposits · no tokens · no objects · no identities');
    // Phase 5 (A21): the line is the heading's right-prompt note, drawn as PromptHeading's note
    // (the count nav stands between them), outside the heading's name.
    const heading = screen.getByRole('heading', { name: /^Holdings / });
    expect(none).toHaveAttribute('data-prompt-note');
    const { container: own } = render(<PromptHeading command="c" note="n" />);
    expect(none.className).toBe(own.querySelector('[data-prompt-note]')!.className);
    expect(heading).not.toContainElement(none);
    expect(screen.queryByRole('heading', { level: 3 })).toBeNull();
    expect(await screen.findByRole('link', { name: 'support.cell' })).toHaveAttribute(
      'href',
      expect.stringContaining('/identities/dotcell/')
    );
    expect(screen.getByText(/These rights do not add/)).toBeInTheDocument();
    // Consistency C3: the role is on this address's participant line (was a "Roles: owner" line
    // under the row, which repeated it).
    const history = screen.getByRole('region', { name: 'History' });
    const party = await waitFor(() => {
      const p = history.querySelector('article[data-feed-row] [data-role="party"]');
      expect(p).not.toBeNull();
      return p as HTMLElement;
    });
    expect(party).toHaveTextContent('owner');
    expect(history).not.toHaveTextContent('Roles: owner');
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
    expect(within(section).getByText('Showing 1–1 names · 20 per page')).toBeInTheDocument();
    fireEvent.click(within(section).getByRole('button', { name: 'Next' }));
    expect(await screen.findByText('next.cell')).toBeInTheDocument();
    expect(api.getAddressDotCellNames).toHaveBeenLastCalledWith(addressHash, {
      limit: 20,
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
        limit: 20,
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
        limit: 20,
        cursor: undefined,
        class: 'identity',
      })
    );
    expect(await within(history).findAllByText(/valar.bit/)).not.toHaveLength(0);
  });
  // contracts W5-1 (BA-35): the tab counts activities — the unfiltered `total`, i.e. the
  // address's transactions minus its cellbases — never `transactionsCount` (1,021 here).
  it('counts the Activities tab from the unfiltered activity total, never the transaction count', async () => {
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.filter === 'all'
        ? { ...pageFixture(addressActivitiesFixture, 1682), hasMore: true, nextCursor: 'next' }
        : pageFixture([])
    );
    render(<AddressDetailPage />);
    await loaded();
    const activitiesTab = () => screen.getByRole('button', { name: /^Activities/ });
    await waitFor(() => expect(activitiesTab()).toHaveTextContent(/^Activities1,682$/));
    expect(activitiesTab()).not.toHaveTextContent('1,021');
    expect(
      await screen.findByText('Showing 1–1 of 1,682 activities · 20 per page')
    ).toBeInTheDocument();
    // A filtered page declares no total; the tab keeps the address's count, the range has none.
    fireEvent.click(screen.getByRole('button', { name: /^DAO/ }));
    expect(await screen.findByText('No DAO activities on this page.')).toBeInTheDocument();
    expect(screen.getByText('Showing 0–0 activities · 20 per page')).toBeInTheDocument();
    expect(activitiesTab()).toHaveTextContent(/^Activities1,682$/);
    await tab('Transactions');
    expect(activitiesTab()).toHaveTextContent(/^Activities1,682$/);
  });
  // F2 blocks (BA-35, R2b-a filter=script): one "Script" chip, as the Address board.
  it('uses one script-call filter and resets the activity cursor', async () => {
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
        limit: 20,
        cursor: 'next-act',
        filter: 'all',
      })
    );
    // C14: each chip carries its count after its label.
    const chips = within(screen.getByRole('group', { name: 'Filter activity' }))
      .getAllByRole('button')
      .map((b) => b.textContent!.replace(/ [\d,]+\+?$/, ''));
    expect(chips).toEqual(['All', 'CKB', 'Token', 'Object', 'Identity', 'DAO', 'Script', 'RGB++']);
    fireEvent.click(screen.getByRole('button', { name: /^Script/ }));
    expect(await screen.findByText('No Script activities on this page.')).toBeInTheDocument();
    expect(api.getAddressActivities).toHaveBeenLastCalledWith(addressHash, {
      limit: 20,
      cursor: undefined,
      filter: 'script',
    });
  });
  it('resets filters, tab and cursors when the route changes', async () => {
    const { rerender } = render(<AddressDetailPage />);
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: /^Token/ }));
    await tab('Transactions');
    route = 'ckb1other';
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      lockScriptHash: fixtureHash(999),
    });
    rerender(<AddressDetailPage />);
    await waitFor(() =>
      expect(api.getAddressActivities).toHaveBeenCalledWith(fixtureHash(999), {
        limit: 20,
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
    expect(await screen.findByText('Showing 1–1 activities · 20 per page')).toBeInTheDocument();
  });
  // C11: the address lists page through the one CursorPagination; its range follows the page size
  // the list asked for (twenty below md), where the old pager counted fifty a page everywhere.
  it('pages the address lists with the shared pager at the phone page size (C11)', async () => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 390 });
    onTestFinished(() => {
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1024 });
    });
    const rows = (from: number) =>
      Array.from({ length: 20 }, (_, i) => ({
        ...addressActivitiesFixture[0],
        txHash: fixtureHash(500 + from + i),
      }));
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.limit === 1
        ? pageFixture([])
        : p?.cursor
          ? { ...pageFixture(rows(20), 45), hasMore: true, nextCursor: 'third' }
          : { ...pageFixture(rows(0), 45), hasMore: true, nextCursor: 'second' }
    );
    render(<AddressDetailPage />);
    await loaded();
    const pages = await screen.findByRole('navigation', { name: 'activities pages' });
    expect(within(pages).getByText('Showing 1–20 of 45 activities · 20 per page')).toBeVisible();
    expect(within(pages).getByText('Page 1 of 3')).toBeVisible();
    fireEvent.click(within(pages).getByRole('button', { name: 'Next' }));
    expect(
      await within(pages).findByText('Showing 21–40 of 45 activities · 20 per page')
    ).toBeVisible();
    expect(within(pages).getByText('Page 2 of 3')).toBeVisible();
    expect(api.getAddressActivities).toHaveBeenLastCalledWith(addressHash, {
      limit: 20,
      cursor: 'second',
      filter: 'all',
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^DAO/ }));
    expect(screen.getByLabelText('Unconfirmed')).toBeInTheDocument();
  });
  it('gives compact and desktop transactions the same pool and partial evidence', async () => {
    // The phone row: the list measures a 358 px container.
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
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

// F2 blocks: the Address / AddressTxs boards (BA-31 … BA-38, BA-45).
describe('Address board · F2 blocks', () => {
  const section = async (name: RegExp) =>
    (await screen.findByRole('heading', { name })).closest('section') as HTMLElement;

  // Lane P (Address.dc.html): the bar is the board's 18 px partition with the shares inside.
  it('keeps every non-zero balance part visible: 3 px minimum, 2 px gaps, 18 px bar (BA-31)', async () => {
    render(<AddressDetailPage />);
    const balance = await screen.findByLabelText('Address capacity partition');
    const parts = Array.from(balance.querySelectorAll('[data-part]')) as HTMLElement[];
    expect(parts.map((p) => p.dataset.part)).toEqual(['occupied', 'free', 'DAO free']);
    for (const part of parts) expect(part).toHaveClass('min-w-[3px]');
    expect(parts[0].parentElement).toHaveClass('h-[18px]', 'gap-0.5');
    expect(balance).toHaveTextContent('free part of 5 deposits');
  });

  it('shows a token type hash only beside names that occur twice (BA-32)', async () => {
    render(<AddressDetailPage />);
    const tokens = await section(/Tokens$/);
    await within(tokens).findAllByText('iCKB');
    const rows = Array.from(tokens.querySelectorAll('[data-token-row]')) as HTMLElement[];
    expect(rows).toHaveLength(9);
    const withHash = rows.filter((row) => row.querySelector('[data-type-hash]'));
    expect(withHash.map((row) => row.dataset.tokenRow)).toEqual([
      fixtureHash(302),
      fixtureHash(306),
    ]);
  });

  it('marks object compositions with the crystal badges on each collection row (BA-33, C14)', async () => {
    render(<AddressDetailPage />);
    const objects = await section(/Objects$/);
    await within(objects).findAllByRole('link', { name: /Object|拯救民主/ });
    expect(
      objects.querySelector('[data-composition="btc_ckb"] [data-composition-mark]')
    ).not.toBeNull();
    expect(
      objects.querySelector('[data-composition="pure_ckb"] [data-composition-mark]')
    ).not.toBeNull();
    expect(objects.querySelector('[data-composition="centralized_mixture"]')).toHaveTextContent(
      'Centralized mix'
    );
  });

  it('orders collections by size, deposits by amount and the space bar by share (BA-34)', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue({
      ...holdingsFixture,
      objectCollections: [...holdingsFixture.objectCollections].reverse(),
    });
    vi.mocked(api.getAddressCells).mockImplementation(async (_addr, p) =>
      pageFixture(
        p?.class === 'identity'
          ? addressIdentitiesFixture
          : p?.class === 'dao'
            ? addressDaoCellsFixture // R2b-a 9: the API answers largest first
            : addressObjectsFixture,
        p?.class === 'identity' ? 1 : 14
      )
    );
    render(<AddressDetailPage />);
    const objects = await section(/Objects$/);
    await waitFor(() => expect(objects.querySelectorAll('[data-collection-row]').length).toBe(7));
    expect(
      Array.from(objects.querySelectorAll('[data-collection-row]')).map(
        (row) => (row as HTMLElement).dataset.collectionRow
      )
    ).toEqual([
      '拯救民主',
      'Nervape',
      'Nervape Mouth',
      'Unicorn',
      'Lightning Explorer Badge',
      'Lightning Pioneer Badge',
      'SOUL Dragon Eggs',
    ]);
    const dao = await section(/Nervos DAO deposits$/);
    await waitFor(() => expect(dao.querySelectorAll('[data-dao-cell]').length).toBe(5));
    expect(
      Array.from(dao.querySelectorAll('[data-dao-cell] [data-ckb-amount]')).map(
        (amount) => amount.textContent?.match(/^([\d,]+)\.0+ CKB$/)?.[1]
      )
    ).toEqual(['5,240,000', '3,500,000', '600,000', '200,000', '50,000']);
    expect(
      Array.from(document.querySelectorAll('[data-space-class]')).map(
        (bar) => (bar as HTMLElement).dataset.spaceClass
      )
    ).toEqual(['object', 'token', 'dao', 'identity', 'script', 'plain']);
  });

  it('dates activity rows instead of "ago" (BA-35)', async () => {
    render(<AddressDetailPage />);
    const history = await screen.findByRole('region', { name: 'History' });
    const evidence = await waitFor(() => {
      const e = history.querySelector('[data-role="evidence"]');
      expect(e).not.toBeNull();
      return e as HTMLElement;
    });
    // Consistency C3: block · date, the site's one row head (was "#… · 0x… · 2026-02-21"); the
    // hash and the age are the row's hover, the headline is the link to the tx.
    expect(evidence).toHaveTextContent(/^#18,672,923 · 2026-02-21$/);
    expect(evidence).not.toHaveTextContent('ago');
    expect(evidence.closest('article')).toHaveAttribute(
      'title',
      expect.stringMatching(new RegExp(`^${addressActivitiesFixture[0].txHash} · \\d+d ago$`))
    );
  });

  it('draws every activity as the shared ActivityRow, this address first and once (C3)', async () => {
    const second = { ...addressActivitiesFixture[0], txHash: fixtureHash(801), ckbDelta: '-5' };
    const page = pageFixture([...addressActivitiesFixture, second]);
    // C14 asks each filter chip for a one-row page; answer it with one row and hasMore, as the
    // API would, so the feed's two rows do not reach the chip's one-row invariant.
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.limit === 1 ? { ...page, data: page.data.slice(0, 1), hasMore: true } : page
    );
    render(<AddressDetailPage />);
    const history = await screen.findByRole('region', { name: 'History' });
    const rows = await waitFor(() => {
      const r = [...history.querySelectorAll<HTMLElement>('article[data-feed-row]')];
      expect(r).toHaveLength(2);
      return r;
    });
    expect(rows.map((r) => r.getAttribute('data-feed-row'))).toEqual([
      addressActivitiesFixture[0].txHash,
      second.txHash,
    ]);
    for (const row of rows) {
      // The endpoint supplies this participant's deltas only: one participant line per row.
      expect(row.querySelectorAll('[data-role="participant"]')).toHaveLength(1);
      expect(within(row).getAllByText('this address')).toHaveLength(1);
      const party = row.querySelector('[data-role="party"]') as HTMLElement;
      expect(within(party).queryByRole('link')).toBeNull();
      expect(within(party).getByTitle(addressText)).toBeInTheDocument();
    }
    expect(within(history).getAllByText('this address')).toHaveLength(2);
  });

  it('marks the Scripts column by class with display names, and dates the Block column (BA-36)', async () => {
    vi.mocked(api.getAddressTransactions).mockResolvedValue(
      pageFixture(
        [{ ...addressTransactionsFixture[0], scriptLabels: ['NervosDAO', 'xUDT', 'Spore'] }],
        1021
      )
    );
    render(<AddressDetailPage />);
    await loaded();
    await tab('Transactions');
    const table = await screen.findByRole('table', { name: 'Address transactions' });
    expect(within(table).getByRole('columnheader', { name: 'Block · date' })).toBeInTheDocument();
    const scripts = within(table).getAllByRole('cell').at(-1)!;
    expect(scripts).toHaveTextContent('Nervos DAOxUDTSpore');
    expect(
      Array.from(scripts.querySelectorAll('[data-class]')).map((m) => m.getAttribute('data-class'))
    ).toEqual(['dao', 'tokens', 'objects']);
    expect(table).toHaveTextContent('2026-02-21');
    expect(table).not.toHaveTextContent(/ago/);
    // Visual pass: the headers carry the unit ("This address, CKB", "Fee, CKB"), rows the number.
    expect(table.querySelector('tbody')).not.toHaveTextContent('CKB');
  });

  it('names the DAO block "Nervos DAO deposits" and its first figure "Deposited" (BA-37)', async () => {
    render(<AddressDetailPage />);
    const dao = await section(/Nervos DAO deposits$/);
    expect(within(dao).getAllByRole('term')[0]).toHaveTextContent('Deposited');
  });

  it('names the lock code and offers JSON and Markdown in the Lock script section (BA-38)', async () => {
    render(<AddressDetailPage />);
    const lock = await section(/^The lock script this address names/);
    expect(lock).toHaveTextContent(/code_hash0x0+64 · JoyID/);
    // Responsive fix plan 2026-11-01 (A3): in the break-all box the lock name split "Def/ault Lock".
    expect(within(lock).getByText('JoyID')).toHaveClass('whitespace-nowrap');
    expect(within(lock).getByRole('link', { name: 'Markdown' }).getAttribute('href')).toBe(
      `/mainnet/address/${addressText}.md`
    );
    expect(within(lock).getByRole('link', { name: 'JSON' }).getAttribute('href')).toMatch(
      new RegExp(`/addresses/${addressText}$`)
    );
  });

  it('draws the lock script as its occupied bytes and derives its two names (C6)', async () => {
    stubChartWidth(600);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    render(<AddressDetailPage />);
    const lock = await section(/^The lock script this address names/);
    const args = (addressFixture.lockScript!.args.length - 2) / 2;
    expect(lock).toHaveTextContent(`Script · ${33 + args} occupied bytes in every cell it locks`);
    expect(
      [...within(lock).getByRole('group', { name: 'lock script byte map' }).children].map(
        (p) => (p as HTMLElement).dataset.segment
      )
    ).toEqual(['code_hash', 'hash_type', 'args']);
    expect(
      within(lock).getByRole('img', { name: `lock script offsets: 0, 32, 33, ${33 + args}` })
    ).toBeInTheDocument();
    expect(within(lock).getByText('hash_type').nextElementSibling).toHaveTextContent(
      `${addressFixture.lockScript!.hashType} (`
    );
    const names = screen.getByRole('region', { name: 'Derived names' });
    expect(names).toHaveTextContent(addressFixture.lockScriptHash);
    expect(names).toHaveTextContent(addressFixture.address!);
  });

  it('writes "1 cell" for a single identity cell (BA-45)', async () => {
    render(<AddressDetailPage />);
    const identities = await section(/Identities$/);
    expect(identities).toHaveTextContent(/1 cell · 227\.99993047 CKB held/);
  });

  // F2 blocks visual pass + R2b-a order=capacity (BA-34).
  it('asks for DAO cells largest first and keeps the API order', async () => {
    vi.mocked(api.getAddressCells).mockImplementation(async (_addr, p) =>
      pageFixture(
        p?.class === 'identity'
          ? addressIdentitiesFixture
          : p?.class === 'dao'
            ? [...addressDaoCellsFixture].reverse()
            : addressObjectsFixture,
        p?.class === 'identity' ? 1 : 14
      )
    );
    render(<AddressDetailPage />);
    const dao = await section(/Nervos DAO deposits$/);
    await waitFor(() => expect(dao.querySelectorAll('[data-dao-cell]').length).toBe(5));
    expect(api.getAddressCells).toHaveBeenCalledWith(addressHash, {
      class: 'dao',
      order: 'capacity',
      limit: 20,
      cursor: undefined,
    });
    expect(
      Array.from(dao.querySelectorAll('[data-dao-cell] [data-ckb-amount]')).map(
        (amount) => amount.textContent?.match(/^([\d,]+)\.0+ CKB$/)?.[1]
      )
    ).toEqual(['50,000', '200,000', '600,000', '3,500,000', '5,240,000']);
  });

  it('puts the activity filters on the tab row, as the board', async () => {
    render(<AddressDetailPage />);
    await loaded();
    const tabs = screen.getByRole('group', { name: 'History views' });
    const row = tabs.parentElement!;
    expect(row).not.toHaveAttribute('aria-label', 'History');
    expect(row).toHaveClass('justify-between');
    expect(row).toContainElement(screen.getByRole('group', { name: 'Filter activity' }));
  });

  it('writes the class headers as the board: "9 kinds in 15 cells", "14 in 7 collections"', async () => {
    render(<AddressDetailPage />);
    expect(await section(/Tokens$/)).toHaveTextContent(
      /9 kinds in 15 cells · 21,507\.60266154 CKB held/
    );
    expect(await section(/Objects$/)).toHaveTextContent(/14 in 7 collections · 4,093\.0+ CKB held/);
  });
});

// Consistency round (Address.dc.html): the h1 is the short address (C13); Holdings collapses the
// classes it does not hold into one line, leads with the composition block and lists one row per
// collection that opens in place to its first ten objects (C14); every filter chip has a count.
describe('Address · consistency round (C13, C14)', () => {
  it('names the page by its short address, the full address and its copy button beneath (C13)', async () => {
    render(<AddressDetailPage />);
    await loaded();
    const h1 = screen.getByRole('heading', {
      level: 1,
      name: `${addressText.slice(0, 12)}…${addressText.slice(-8)}`,
    });
    expect(h1).toHaveClass('text-entity');
    const header = h1.closest('header')!;
    expect(header).toHaveTextContent(addressText);
    expect(within(header).getByRole('button', { name: 'Copy address' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { level: 1, name: 'Address' })).toBeNull();
  });

  it('collapses only the classes it does not hold into the line by the heading', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue({
      ...holdingsFixture,
      holdings: { ...holdingsFixture.holdings, token: 0 },
    });
    vi.mocked(api.getAddressTokens).mockResolvedValue(pageFixture([]));
    render(<AddressDetailPage />);
    await loaded();
    const holdings = screen.getByRole('heading', { name: 'Holdings 40 cells' }).closest('section')!;
    expect(within(holdings).getByText('no tokens')).toBeInTheDocument();
    expect(within(holdings).queryByRole('heading', { name: /Tokens$/ })).toBeNull();
    expect(within(holdings).getByRole('heading', { name: /Objects$/ })).toBeInTheDocument();
    expect(api.getAddressTokens).not.toHaveBeenCalled();
  });

  it('leads Holdings with the composition block, every tier counted', async () => {
    render(<AddressDetailPage />);
    await loaded();
    const holdings = screen.getByRole('heading', { name: 'Holdings 40 cells' }).closest('section')!;
    const comp = within(holdings).getByRole('group', { name: 'Object composition' });
    expect(comp).toHaveTextContent('11 of 14 fully on-chain');
    expect(
      [...comp.querySelectorAll('li')].map((li) => [
        li.dataset.tier,
        li.querySelector('[data-count]')!.textContent,
      ])
    ).toEqual([
      ['pure_ckb', '1'],
      ['btc_ckb', '10'],
      ['decentralized_mixture', '0'],
      ['centralized_mixture', '3'],
      ['unknown', '0'],
    ]);
  });

  it('lists one row per collection and opens one in place to its first ten objects', async () => {
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) =>
      p?.collection
        ? {
            ...pageFixture(
              addressObjectsFixture.filter((c) => c.display!.collectionId === p.collection)
            ),
            limit: 10,
          }
        : pageFixture(p?.class === 'identity' ? addressIdentitiesFixture : addressDaoCellsFixture)
    );
    render(<AddressDetailPage />);
    await loaded();
    const objects = (await screen.findByRole('heading', { name: /Objects$/ })).closest('section')!;
    const rows = [...objects.querySelectorAll<HTMLElement>('[data-collection-row]')];
    expect(rows.map((r) => r.dataset.collectionRow)).toEqual([
      '拯救民主',
      'Nervape',
      'Nervape Mouth',
      'Unicorn',
      'Lightning Explorer Badge',
      'Lightning Pioneer Badge',
      'SOUL Dragon Eggs',
    ]);
    expect(rows[0]).toHaveTextContent('6');
    // Nothing is listed (or asked for) until a collection is opened.
    expect(api.getAddressCells).not.toHaveBeenCalledWith(
      addressHash,
      expect.objectContaining({ class: 'object' })
    );
    const open = within(rows[0]).getByRole('button', { name: 'Open 拯救民主' });
    expect(open).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(open);
    await waitFor(() =>
      expect(api.getAddressCells).toHaveBeenCalledWith(addressHash, {
        class: 'object',
        collection: fixtureHash(200),
        limit: 10,
        cursor: undefined,
      })
    );
    const list = await screen.findByRole('table', { name: '拯救民主 objects held here' });
    expect(within(list).getAllByRole('row')).toHaveLength(7);
    expect(
      within(list).getByRole('link', { name: `Object ${addressObjectsFixture[0].display!.id}` })
    ).toHaveAttribute(
      'href',
      expect.stringContaining(`/objects/${addressObjectsFixture[0].display!.id}`)
    );
    expect(screen.getByRole('link', { name: 'All 6 in the collection →' })).toHaveAttribute(
      'href',
      expect.stringContaining(`/clusters/${fixtureHash(200)}`)
    );
    expect(objects).not.toHaveTextContent('Items are on another page');
    fireEvent.click(within(rows[0]).getByRole('button', { name: 'Close 拯救民主' }));
    expect(screen.queryByRole('table', { name: '拯救民主 objects held here' })).toBeNull();
  });

  // Lane P (Address.dc.html): the balance hero, then the four metrics, then Holdings.
  it('leads with the balance figure and its partition, then the four metrics, then Holdings', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    const tier1 = container.querySelector('[data-tier="1"]')!;
    expect(tier1.querySelector('section')).toHaveAttribute('aria-label', 'Balance');
    const balance = screen.getByRole('region', { name: 'Balance' });
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    const holdings = screen.getByRole('heading', { name: 'Holdings 40 cells' });
    expect(
      balance.compareDocumentPosition(metrics) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(
      metrics.compareDocumentPosition(holdings) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(balance.querySelector('[data-role="balance-figure"]')).toHaveTextContent(
      /^9,651,178\.35338057 CKB$/
    );
    expect([...metrics.querySelectorAll('dt')].map((d) => d.textContent)).toEqual([
      'Holdings',
      'Live cells',
      'Transactions',
      'Lock',
    ]);
  });
  // Lane P (exact or none): a filter chip shows a count only when its one-row page proves it — 0
  // or 1 — and no number otherwise; "All" keeps the address's total; an empty filter is dimmed.
  it('counts a filter chip from its one-row page only when the count is exact; an empty filter is dimmed 0', async () => {
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.limit === 1 && p.filter && p.filter !== 'all'
        ? p.filter === 'token'
          ? { ...pageFixture([addressActivitiesFixture[0]]), hasMore: false, total: undefined }
          : p.filter === 'ckb'
            ? { ...pageFixture([addressActivitiesFixture[0]]), hasMore: true, total: undefined }
            : { ...pageFixture([]), hasMore: false, total: undefined }
        : // the unfiltered page states the address's activity count (contracts W5-1)
          pageFixture(addressActivitiesFixture, addressActivitiesFixture.length)
    );
    render(<AddressDetailPage />);
    await loaded();
    const chips = screen.getByRole('group', { name: 'Filter activity' });
    await waitFor(() =>
      expect(within(chips).getByRole('button', { name: /^DAO/ })).toHaveTextContent('DAO 0')
    );
    expect(within(chips).getByRole('button', { name: /^All/ })).toHaveTextContent(
      `All ${addressActivitiesFixture.length}`
    );
    expect(within(chips).getByRole('button', { name: /^Token/ })).toHaveTextContent(/^Token 1$/);
    // More than the one row asked for: the API counts no filtered total, so the chip shows no
    // number (never "2+"); what the page proved stays in the chip's title.
    const ckb = within(chips).getByRole('button', { name: /^CKB/ });
    expect(ckb).toHaveTextContent(/^CKB$/);
    expect(ckb.querySelector('[data-chip-count]')).toBeNull();
    expect(ckb).toHaveAttribute(
      'title',
      'more than one; the API keeps no count of filtered activity'
    );
    expect(chips).not.toHaveTextContent('2+');
    for (const chip of within(chips).getAllByRole('button'))
      if (chip !== ckb) expect(chip).toHaveTextContent(/\d$/);
    expect(within(chips).getByRole('button', { name: /^DAO/ })).toHaveClass('text-text-ghost');
    expect(api.getAddressActivities).toHaveBeenCalledWith(addressHash, {
      limit: 1,
      cursor: undefined,
      filter: 'dao',
    });
  });
});

// Lane A follow-up 2 (C14): under "On-chain space they take" the legend lists only the classes this
// address holds; the rest is one quiet line, their zero counts kept in its title.
describe('Address · space legend (lane A follow-up)', () => {
  const objectsAndPlain: AddressHoldings = {
    ...holdingsFixture,
    balance: '3844275078856',
    occupiedCapacity: '388700000000',
    freeCapacity: '3455575078856',
    daoFreeCapacity: '0',
    liveCellsCount: 16,
    depositCellsCount: 0,
    withdrawalRequestCellsCount: 0,
    daoUnclaimedCompensation: '0',
    estimatedApc: null,
    holdings: { dao: 0, token: 0, object: 14, identity: 0 },
    classes: {
      dao: { cellCount: 0, capacity: '0', occupiedCapacity: '0' },
      token: { cellCount: 0, capacity: '0', occupiedCapacity: '0' },
      object: holdingsFixture.classes.object,
      identity: { cellCount: 0, capacity: '0', occupiedCapacity: '0' },
      script: { cellCount: 0, capacity: '0', occupiedCapacity: '0' },
      plain: holdingsFixture.classes.plain,
    },
  };
  it('keeps only the classes it holds, and says the rest in one line', async () => {
    vi.mocked(api.getAddressHoldings).mockResolvedValue(objectsAndPlain);
    render(<AddressDetailPage />);
    await loaded();
    const legend = document.querySelector<HTMLElement>('[data-space-legend]')!;
    expect(
      within(legend)
        .getAllByRole('button')
        .map((b) => b.textContent)
    ).toEqual(['Objects3,76196.76%', 'Plain CKB1263.24%']);
    const rest = within(legend).getByText('nothing in DAO, tokens, identities or other scripts');
    expect(rest).toHaveClass('text-text-dim');
    expect(rest).toHaveAttribute(
      'title',
      'Nervos DAO 0 CKB · Tokens 0 CKB · Identities 0 CKB · Other scripts 0 CKB'
    );
    expect(
      [...document.querySelectorAll<HTMLElement>('[data-space-class]')].map(
        (b) => b.dataset.spaceClass
      )
    ).toEqual(['object', 'plain']);
    // The composition legend keeps its five tiers, zeros included.
    expect(
      screen.getByRole('group', { name: 'Object composition' }).querySelectorAll('li')
    ).toHaveLength(5);
  });
  it('writes no line when it holds every class', async () => {
    render(<AddressDetailPage />);
    await loaded();
    const legend = document.querySelector<HTMLElement>('[data-space-legend]')!;
    expect(within(legend).getAllByRole('button')).toHaveLength(6);
    expect(within(legend).queryByText(/^nothing in/)).toBeNull();
  });
});
