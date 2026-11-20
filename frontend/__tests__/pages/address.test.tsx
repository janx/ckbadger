import { describe, it, expect, vi, beforeEach, onTestFinished } from 'vitest';
import { chooseView } from '@/__tests__/utils/tab-row';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
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
import { fullText } from '@/__tests__/utils/full-text';
import { factsOf } from '@/__tests__/utils/facts';
import { expectAnswerUnclipped } from '@/__tests__/utils/copy-answer';
import { pagedRead } from '@/__tests__/utils/paged-read';
import { activityRead } from '@/__tests__/utils/activity-read';
import { inkOf, READING } from '@/__tests__/utils/ink-role';
import { typeStepOf } from '@/__tests__/utils/type-step-of';
import { formatUtc8Date } from '@/lib/utils';

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
// a pool row states no item types, null and null, as the route sends it (the third read's T
// SHOULD-1: it inherited the committed row's [])
const poolActivity = (): Activity =>
  activityRead({
    ...addressActivitiesFixture[0],
    txHash: fixtureHash(900),
    blockNumber: null,
    txIndex: null,
    timestamp: null,
    poolStatus: 'proposed',
    timeAddedToPool: new Date(Date.now() - 120000).toISOString(),
    ckbDelta: '-50000000000',
    interpretation: { status: 'complete' },
    itemTypes: null,
    itemTypesUnread: null,
  });
const emptyHoldings: AddressHoldings = {
  ...holdingsFixture,
  balance: '0',
  occupiedCapacity: '0',
  daoCellsCapacity: '0',
  otherCellsOccupiedCapacity: '0',
  otherCellsFreeCapacity: '0',
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
// re-pinned (lane c2-chain): the history's views are the library's tab row (were pressed buttons);
// re-pinned on purpose (the lead's final round, item 4): through the row's form on its line, a tab
// or, on a stubbed line that cannot hold the names, the row's Select
const tab = async (name: string) => chooseView(new RegExp(`^${name}`, 'i'));
// Phase 5 (A21): the holdings heading runs on the address's live cells (U-16, re-pinned: the count
// and its noun are one unit, a no-break space between them).
const loaded = () => screen.findByRole('heading', { name: /^Holdings [\d,]+\scells?$/ });
/** `n` distinct live cells, the address's object cells repeated, numbered from `from` (a page of
 * them agrees with itself: pagedRead). */
/** `n` distinct activities, the address's repeated with their own tx hashes, from `from`. */
const activitiesPage = (n: number, from = 0) =>
  Array.from({ length: n }, (_, i) => ({
    ...addressActivitiesFixture[(from + i) % addressActivitiesFixture.length],
    txHash: `0x${(4096 + from + i).toString(16).padStart(64, '0')}`,
  }));
const liveCellsPage = (n: number, from = 0) =>
  Array.from({ length: n }, (_, i) => ({
    ...addressObjectsFixture[(from + i) % addressObjectsFixture.length],
    outputIndex: 1000 + from + i,
  }));

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
  // Lane D2 (PA-13; gate D0′'s strip, with the address's own four facts): the lock, the live
  // cells, the transactions and the holdings stand in the status strip under the id line. They
  // were said twice: on the kind line and again in the metrics row under the balance.
  it('states the lock, live cells, transactions and holdings in one strip under the id line (lane D2)', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    const strip = await screen.findByRole('group', { name: 'Address status' });
    await waitFor(() =>
      expect(factsOf(strip).map(([label]) => label)).toEqual([
        'lock',
        'live cells',
        'transactions',
        'holdings',
      ])
    );
    expect(factsOf(strip).map(([, value]) => value)).toEqual([
      'JoyID · type · 22-byte args',
      '40',
      '1,021',
      // re-pinned on purpose (lane P1; review of phases C and D, NOTE 12): no sum of unlike counts
      '5 DAO cells · 9 tokens · 14 objects · 1 identity',
    ]);
    expect(within(strip).getByRole('link', { name: 'JoyID' })).toHaveAttribute(
      'href',
      expect.stringContaining('/scripts/JoyID')
    );
    // under the stone and the plate; the kind line holds the kind word alone
    const { kindLine, below } = expectNamePlate(container, 'Address');
    expect(below!.firstElementChild).toBe(strip);
    expect(kindLine).toHaveTextContent(/^Address$/);
    // said once: no metrics row repeats them
    expect(screen.queryByRole('region', { name: 'Metrics' })).toBeNull();
  });

  it('renders the real address Fiber endpoint empty array without a page crash', async () => {
    vi.mocked(api.getAddressFiberChannels).mockResolvedValue([]);
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.queryByRole('heading', { name: /^Fiber channels/ })).not.toBeInTheDocument();
    // Consistency plan C1: the tab names the page.
    await waitFor(() =>
      expect(document.title).toMatch(/^ck[bt]1…[0-9a-z]{6} · address · CKBadger$/)
    );
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
    expect(await screen.findByText('nothing at this address.')).toBeInTheDocument();
    // U-18: the checked endpoint stands in the facts, the address shortened, once; R9 (re-pinned):
    // shortened as an address, `ckb1…` and its last 8 (it was cut as a hash, 10…6)
    expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
      `/addresses/${addressText.slice(0, 4)}…${addressText.slice(-8)}`
    );
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

  // The name plate (seals by kind, C10): beside the lock script's stone, the kind line "Address",
  // the short address, then the full address and its copy button. Re-pinned on purpose (lane D2;
  // renamed, it was "…: Address and its status, the short address, the full one"): the page's
  // status (the lock, the live cells, the transactions) is the status strip under them, with the
  // holdings, so the kind line holds the kind word alone and the strip stands under the plate
  // (nothing stood there; the address page has no lede).
  it('stands the address’s name plate beside the stone: Address, the short address, the full one', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    const { stone, kindLine, nameBox, title, below } = expectNamePlate(container, 'Address');
    expect(stone).toHaveAttribute('data-seal', 'script');
    expect(below!.children).toHaveLength(1);
    expect(below!.firstElementChild).toBe(screen.getByRole('group', { name: 'Address status' }));
    expect(kindLine.children).toHaveLength(1);
    expect(kindLine).toHaveTextContent(/^Address$/);
    expect(within(below!).getByRole('link', { name: 'JoyID' })).toHaveAttribute(
      'href',
      expect.stringContaining('/scripts/JoyID')
    );
    // R9 (re-pinned): the h1's head size keeps the network and the last 8 characters
    expect(title).toHaveTextContent(
      new RegExp(`^${addressText.slice(0, 4)}…${addressText.slice(-8)}$`)
    );
    expect(title).toHaveClass('text-entity', 'max-md:text-index');
    expect(nameBox.children).toHaveLength(2);
    const idLine = nameBox.lastElementChild as HTMLElement;
    expect(idLine).toHaveTextContent(new RegExp(`^${addressText}$`));
    expect(idLine).toContainElement(screen.getByRole('button', { name: 'copy address' }));
    // C10 round 2: the id line in the plate's one voice, set by the plate: the full address
    // (it was text-body in the text ink) takes it; the copy button keeps its own classes
    // re-pinned with readability ruling R3: one voice still, in the reading ink (the id is a
    // value; it was text-text-dim)
    expect(idLine.className).toBe('text-text text-small min-w-0 [overflow-wrap:anywhere]');
    // A4 Chrome F3 (re-pinned): a full id in a head is the head's HeadId, one line where the
    // measured line holds it, else equal unbreakable lines — never the own-box scroller it was
    // (box 328 px for 698 px at 390)
    const full = idLine.querySelector('[data-head-id]') as HTMLElement;
    expect(full).not.toBeNull();
    expect(full.textContent).toBe(addressText);
    expect(full).toHaveAttribute('title', addressText);
    expect(full.className).not.toMatch(/(^|\s)text-/);
    expect(idLine.querySelector('.overflow-x-auto')).toBeNull();
    expect(full.parentElement!.className).toBe('flex min-w-0 items-start gap-2');
    // Re-pinned on purpose (lane c2-chain): the copy is the library's CopyButton, the board's one
    // icon in the quiet ink with its answer in words beside it, the pair kept whole beside the id
    // (was a bare button whose name said what happened, "Address copied", and drew a ✓).
    // Re-pinned again (lane c2-finish, rulings round 1): the answer is no longer beside the icon in
    // the line (it took 6 px and its words, 43.2 or 72 px, from the address's line while it
    // stood); it hangs under the whole id out of the line's flow, the copy the row's full height.
    const copy = screen.getByRole('button', { name: 'copy address' });
    expect(copy).toHaveClass('text-text-dim', 'hover:text-jade');
    expect(copy.parentElement).toHaveClass('shrink-0', 'self-stretch');
    const answer = within(copy.parentElement!).getByRole('status');
    expect(answer).toBeEmptyDOMElement();
    expect(answer).toHaveClass('absolute', 'top-full');
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

  // U-06, PA-01, R11: a 400 says the address is not one: the voice page with its own first
  // line and the API's reason, not a grey read-error line (nor the 404's "no address").
  it('answers an invalid address 400 with the not-an-id page and the API’s reason', async () => {
    vi.mocked(api.getAddress).mockRejectedValue(
      new ApiRequestError(400, 'bad_request', 'invalid address')
    );
    render(<AddressDetailPage />);
    expect(await screen.findByText('that is not a ckb address.')).toBeInTheDocument();
    expect(document.querySelector('.error-facts')).toHaveTextContent('invalid address');
    expect(screen.queryByText('nothing at this address.')).not.toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
  });
  it('says a failed address read in the one shared line', async () => {
    vi.mocked(api.getAddress).mockRejectedValue(
      new ApiRequestError(500, 'internal_error', 'addr index mismatch')
    );
    render(<AddressDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /^Could not read this address: API error: 500 - addr index mismatch$/
    );
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
    // the balance figure in the summary form, exact in its title (readability R5, re-pinned on
    // purpose: was every decimal)
    expect(screen.getByLabelText('Balance')).toHaveTextContent('balance9,651,178 CKB');
    // re-pinned on purpose (lane P1, third round; the lead's ruling on review p-logic-chain MUST
    // 3): beside the balance its unclaimed compensation alone; its free part is one of the three
    // parts under the bar (it stood beside the balance as "spendable")
    expect(
      screen.getByLabelText('Balance').querySelector('[data-role="balance-beside"]')
    ).not.toHaveTextContent('spendable');
    // the exact parts, exact form (readability R5, re-pinned on purpose: was padded); re-pinned with
    // readability R4 (lane B7): a labelled fact (was the parts line's last item after "·")
    expect(
      factsOf(
        within(screen.getByLabelText('Address capacity partition')).getByTestId('balance-parts')
      )
      // re-pinned on purpose (lane P1, third round; SHOULD 3): its DAO cells whole (it was "in the
      // DAO 9,589,480 CKB, free part of 5 deposits", their free part alone)
    ).toContainEqual(['in Nervos DAO cells', '9,590,000 CKB in 5 deposits']);
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
      // re-pinned (A4 round 2): each count a unit, its number and noun joined by a plain space
    ).toEqual(['5 DAO cells', '9 tokens', '14 objects', '1 identity']);
    // PA-25 (the lead's follow-up): the nav opened with a loose "·" that was left alone at a line's
    // start when the nav wrapped under the heading. The heading and the links are one separated
    // list: each link carries its own dot, none is loose text.
    const list = nav.closest('[data-dotted]');
    expect(list).not.toBeNull();
    expect(list).toContainElement(screen.getByRole('heading', { name: 'Holdings 40 cells' }));
    expect(nav.querySelectorAll('[data-dotted-separator]')).toHaveLength(4);
    const loose = [...nav.querySelectorAll('*'), nav].flatMap((el) =>
      [...el.childNodes].filter(
        (n) =>
          n.nodeType === Node.TEXT_NODE &&
          /·/.test(n.textContent ?? '') &&
          !(el as Element).hasAttribute('data-dotted-separator')
      )
    );
    expect(loose).toHaveLength(0);
    // Phase 5 (A21): the holdings run on the live cells; the count nav stands beside the heading,
    // outside its name, and the section is named by the heading.
    const heading = screen.getByRole('heading', { name: 'Holdings 40 cells' });
    expectPrompt(heading, 'Holdings', '40 cells');
    // re-pinned (PA-25): the heading and the nav are items of one separated list
    expect(heading.closest('[data-dotted]')).toContainElement(nav);
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
          : // a full page of two with more after it (lane cd-fixes-controls, review-p-tests SHOULD-5: two on a page of
            // twenty that said more followed)
            pagedRead({
              ...pageFixture(addressObjectsFixture.slice(0, 2)),
              limit: 2,
              hasMore: true,
              nextCursor: 'objects-next',
            })
    );
    render(<AddressDetailPage />);
    await loaded();
    // C8: the composition block counts every object; an opened collection pages its own.
    expect(screen.getByRole('group', { name: 'Object composition' })).toHaveTextContent(
      '11 of 14 fully on-chain'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open 拯救民主' }));
    // Re-pinned on purpose (lane c2-chain): the library's pager is counted of the collection's
    // objects this address holds (6) at ten a page, one page, so it draws nothing however the
    // page answers (the old pager drew "Showing 1–2 of 6 objects · 10 per page" from the
    // mock's hasMore); the collection's total stays apart from the page's two rows on the way
    // onward to it.
    // Re-pinned again (lane c2-finish, 1c): the 6 is the holdings' count, another read; the page
    // is counted only by its own response, which counts no rows and says there is more, so the
    // pager draws its rows, its page and a way on (the page's "more" is the API's word on it).
    const table = await screen.findByRole('table', { name: '拯救民主 objects held here' });
    expect(within(table).getAllByRole('row')).toHaveLength(3);
    expect(screen.getByRole('link', { name: 'All 6 in the collection →' })).toBeInTheDocument();
    const objectsPager = screen.getByRole('navigation', { name: '拯救民主 objects' });
    expect(within(objectsPager).getByText('page 1')).toBeInTheDocument();
    expect(objectsPager).not.toHaveTextContent(/ of /);
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
        : // a full page of the nine kinds with more after it (lane cd-fixes-controls, review-p-tests SHOULD-5: nine on a page
          // of twenty that said more followed)
          pagedRead({
            ...pageFixture(addressTokensFixture),
            limit: addressTokensFixture.length,
            hasMore: true,
            nextCursor: 'next-token',
          })
    );
    render(<AddressDetailPage />);
    // re-pinned (lane c2-chain): the library's pager, its range as the board writes it (was
    // "Showing 1–9 of 101 token kinds · 20 per page" and a Next button). Re-pinned again (lane
    // c2-finish, 1c): the 101 was the holdings' read, at another tip than the page; the page's own
    // response counts no rows, so the pager draws its rows and its page, the next by its "more",
    // and the 101 stands where the page states it, the holdings' count of token kinds.
    const pages = await screen.findByRole('navigation', { name: 'token kinds' });
    // re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-5): the page is a full page of nine (it said 20 a page)
    expect(pages).toHaveTextContent(/^1–9 · 9 a page/);
    expect(within(pages).getByText('page 1')).toBeInTheDocument();
    expect(pages).not.toHaveTextContent(/of 101|page 1 of/);
    const section = screen.getByRole('heading', { name: /Tokens/ }).closest('section')!;
    fireEvent.click(within(section).getByRole('button', { name: 'next →' }));
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
    // Lane P: the name links to the script. Re-pinned on purpose (lane D2): said once, as the
    // status strip's lock (it headed the status line and the Lock metric, two links), and the
    // deprecation is the lock's note, lowercase in the warning's ink (it was "Deprecated" on the
    // kind line)
    const links = screen.getAllByRole('link', { name: 'JoyID' });
    expect(links).toHaveLength(1);
    for (const link of links)
      expect(link).toHaveAttribute('href', expect.stringContaining('/scripts/JoyID'));
    const strip = screen.getByRole('group', { name: 'Address status' });
    expect(within(strip).getByText('deprecated')).toHaveClass('text-negative');
  });
  // Lane P1 follow-up (the lock-words gate's known writer 11, the lock byte map's name and link):
  // the address's lock is named once, by the one decision, and the strip and the byte map write
  // it. A lock on the default lock's code hash by data is not the default lock: the registry's name
  // for that code hash ("SECP256K1/blake160") is written nowhere, and no link opens the default
  // lock's named page.
  it('writes no default lock name for a lock on its code hash by data, strip or byte map', async () => {
    const args = `0x${'ab'.repeat(20)}`;
    vi.mocked(api.getAddress).mockResolvedValue({
      ...addressFixture,
      address: ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'data', args }),
      lockScript: { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'data', args },
      lockScriptInfo: {
        codeHash: DEFAULT_LOCK_CODE_HASH,
        name: 'SECP256K1/blake160',
        scriptKind: 'lock',
        deprecated: false,
      },
    });
    render(<AddressDetailPage />);
    await loaded();
    expect(
      screen.getByRole('heading', { name: 'The lock script this address names' })
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent('SECP256K1/blake160');
    expect(document.querySelector('a[href*="/scripts/"]')).toBeNull();
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
    expect(screen.queryByText(fullText('0 holdings, one mark each'))).not.toBeInTheDocument();
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
        : // a full page of one with more after it (lane cd-fixes-controls, review-p-tests SHOULD-5)
          pagedRead({
            ...pageFixture([namedRight]),
            limit: 1,
            hasMore: true,
            nextCursor: 'next-name',
          })
    );
    render(<AddressDetailPage />);
    const section = await screen.findByTestId('address-dotcell-names');
    // Re-pinned on purpose (lane c2-chain): the names are a short list the API counts no total
    // of, so they take the library pager's short-list form, `next 20 ↓`, which adds the next
    // names under the ones shown (was "Showing 1–1 names · 20 per page" and a Next button that
    // replaced them); still no count of one page is said as a total.
    expect(section).not.toHaveTextContent(/of \d|Showing/);
    fireEvent.click(within(section).getByRole('button', { name: 'next 20 ↓' }));
    expect(await screen.findByText('next.cell')).toBeInTheDocument();
    expect(within(section).getByText('support.cell')).toBeInTheDocument();
    expect(api.getAddressDotCellNames).toHaveBeenLastCalledWith(addressHash, {
      limit: 20,
      cursor: 'next-name',
    });
    // the last load has no more after it: the way onward goes
    expect(within(section).queryByRole('button', { name: 'next 20 ↓' })).toBeNull();
  });
  it('omits named rights when none are present', async () => {
    render(<AddressDetailPage />);
    await loaded();
    expect(screen.queryByTestId('address-dotcell-names')).not.toBeInTheDocument();
  });
  // Lane cd-fixes-controls, item 1 (L1 MUST 1): an address whose live cells are not a multiple of
  // the page size. `next →` moves the walk to page 2 at once while the list keeps page 1's
  // response until page 2's arrives (keepPreviousData); the pager read page 1's response as page
  // 2, rows 21–40 of 25, threw, and the address page was the crash page. The page a response was
  // asked for travels with it: while page 2 loads the pager says page 1's figures and offers no
  // next; when page 2 arrives, its own.
  // Lane cd-fixes-controls, the second read's MUST 1: an address with a pending transaction opens
  // its own page. Page one of its activities and of its transactions is the pool's rows above up
  // to 20 committed rows, the page size stated as asked; the pager counted every row against it
  // and threw ("activities: 21 rows on a page of 20"), and the address page was the error page.
  // The pool's rows are page one's own part: 1 pool row over 20 committed rows is page one of
  // more, 2 pool rows over 19 committed rows one page with no pager.
  describe('page one with the pool’s rows above its committed rows', () => {
    const pooled = (n: number) =>
      Array.from({ length: n }, (_, i) => ({ ...poolActivity(), txHash: fixtureHash(900 + i) }));
    const pooledTxs = (n: number) =>
      Array.from({ length: n }, (_, i) => ({
        ...addressTransactionsFixture[0],
        txHash: fixtureHash(950 + i),
        blockNumber: null,
        timestamp: null,
        poolStatus: 'pending' as const,
        timeAddedToPool: new Date(Date.now() - 60000).toISOString(),
      }));
    const committedTxs = (n: number) =>
      Array.from({ length: n }, (_, i) => ({
        ...addressTransactionsFixture[i % addressTransactionsFixture.length],
        txHash: `0x${(8192 + i).toString(16).padStart(64, '0')}`,
      }));
    const pageOne = <T,>(pool: T[], rows: T[], more: boolean) =>
      pagedRead({
        ...pageFixture([...pool, ...rows]),
        hasMore: more,
        nextCursor: more ? 'older-1' : null,
      });

    it.each([
      [1, 20, true],
      [2, 19, false],
    ] as const)(
      'opens its activities with %i pool rows above %i committed rows',
      async (pooledCount, rows, more) => {
        vi.mocked(api.getAddressActivities).mockImplementation(async (_l, p) =>
          p?.filter !== 'all' || p.cursor
            ? pagedRead(pageFixture([]))
            : { ...pageOne(pooled(pooledCount), activitiesPage(rows), more), pool }
        );
        render(<AddressDetailPage />);
        await loaded();
        const history = screen.getByRole('region', { name: 'History' });
        // the pool's rows stand above the committed ones, each with its status
        expect((await within(history).findAllByText('Proposed')).length).toBeGreaterThanOrEqual(
          pooledCount
        );
        if (more)
          expect(await within(history).findByRole('button', { name: 'older →' })).toBeVisible();
        else expect(within(history).queryByRole('navigation', { name: 'activities' })).toBeNull();
      }
    );

    it.each([
      [1, 20, true],
      [2, 19, false],
    ] as const)(
      'opens its transactions with %i pool rows above %i committed rows',
      async (pooledCount, rows, more) => {
        vi.mocked(api.getAddressTransactions).mockImplementation(async (_l, p) =>
          p?.cursor
            ? pagedRead(pageFixture([]))
            : { ...pageOne(pooledTxs(pooledCount), committedTxs(rows), more), pool }
        );
        render(<AddressDetailPage />);
        await loaded();
        await tab('Transactions');
        const table = await screen.findByRole('table', { name: 'Address transactions' });
        expect(within(table).getAllByRole('row').length).toBeGreaterThan(rows);
        expect(within(table).getAllByText('Pending').length).toBeGreaterThanOrEqual(pooledCount);
        const history = screen.getByRole('region', { name: 'History' });
        if (more)
          expect(await within(history).findByRole('button', { name: 'older →' })).toBeVisible();
        else expect(within(history).queryByRole('navigation', { name: 'transactions' })).toBeNull();
      }
    );
  });

  // Lane cd-fixes-controls, the second read's SHOULD 1: Activities on "all", the reader picks
  // "dao" and, before dao's page arrives, presses `older →`. The page still held "all"'s page one,
  // whose stamp matched the reset walk, so `older →` walked dao from "all"'s 20th row as "page 2".
  // The walk knows its list: "all"'s page, still shown, is another list's, with no way on.
  it('offers no way on from the old kind’s page while the new kind’s page loads', async () => {
    let arrive: () => void = () => {};
    const dao = new Promise<void>((resolve) => (arrive = resolve));
    vi.mocked(api.getAddressActivities).mockImplementation(async (_l, p) => {
      if (p?.limit !== 20) return pagedRead(pageFixture([]));
      if (p.filter === 'dao') {
        await dao;
        return pagedRead(pageFixture(activitiesPage(5, 40)));
      }
      return pagedRead({ ...pageFixture(activitiesPage(20)), hasMore: true, nextCursor: 'all-20' });
    });
    render(<AddressDetailPage />);
    await loaded();
    const history = screen.getByRole('region', { name: 'History' });
    expect(await within(history).findByRole('button', { name: 'older →' })).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: /^dao/ }));
    await waitFor(() =>
      expect(api.getAddressActivities).toHaveBeenLastCalledWith(
        addressHash,
        expect.objectContaining({ filter: 'dao', limit: 20, cursor: undefined })
      )
    );
    // "all"'s page one stands while dao's loads: drawn as its own, with no way on and no way back
    expect(within(history).queryByRole('button', { name: 'older →' })).toBeNull();
    expect(within(history).queryByRole('button', { name: /newer/ })).toBeNull();
    await act(async () => arrive());
    await waitFor(() =>
      expect(within(history).queryByRole('navigation', { name: 'activities' })).toBeNull()
    );
    expect(api.getAddressActivities).not.toHaveBeenCalledWith(
      addressHash,
      expect.objectContaining({ filter: 'dao', cursor: 'all-20' })
    );
  });

  it('steps into the last page of 25 live cells while page 2 is still on its way', async () => {
    let arrive: () => void = () => {};
    const second = new Promise<void>((resolve) => (arrive = resolve));
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) => {
      if (p?.class) return pagedRead(pageFixture([]));
      if (!p?.cursor)
        return pagedRead({
          ...pageFixture(liveCellsPage(20), 25),
          hasMore: true,
          nextCursor: 'cell-20',
        });
      await second;
      return pagedRead(pageFixture(liveCellsPage(5, 20), 25), 20);
    });
    render(<AddressDetailPage />);
    await loaded();
    await tab('Live cells');
    const history = screen.getByRole('region', { name: 'History' });
    const pages = await within(history).findByRole('navigation', { name: 'live cells' });
    expect(pages).toHaveTextContent(/^1–20 of 25 · 20 a page/);
    fireEvent.click(within(pages).getByRole('button', { name: 'next →' }));
    // page 2 is asked for; page 1's rows stand, and the pager says page 1, with no way on
    await waitFor(() =>
      expect(api.getAddressCells).toHaveBeenLastCalledWith(addressHash, {
        limit: 20,
        cursor: 'cell-20',
        class: undefined,
      })
    );
    const kept = within(history).getByRole('navigation', { name: 'live cells' });
    expect(kept).toHaveTextContent(/^1–20 of 25/);
    expect(within(kept).getByText('page 1 of 2')).toBeInTheDocument();
    expect(within(kept).queryByRole('button', { name: 'next →' })).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
    await act(async () => arrive());
    const last = await within(history).findByText('page 2 of 2');
    expect(last.closest('nav')).toHaveTextContent(/^21–25 of 25/);
  });

  // Lane P1 (the placement lane's note: the class filter appeared only once the holdings had
  // loaded, its counts coming from them, and pushed the list down when it did): the filter stands
  // from the first draw, its classes without counts until the holdings answer, so nothing jumps.
  it('draws the class filter before the holdings answer, its counts added in place (P1)', async () => {
    let answer: (h: typeof holdingsFixture) => void = () => {};
    vi.mocked(api.getAddressHoldings).mockReturnValue(
      new Promise<typeof holdingsFixture>((resolve) => {
        answer = resolve;
      })
    );
    vi.mocked(api.getAddressCells).mockResolvedValue(pageFixture(addressObjectsFixture));
    render(<AddressDetailPage />);
    await tab('Live cells');
    const filter = await screen.findByRole('radiogroup', { name: 'class' });
    expect(within(filter).getByRole('radio', { name: /^objects$/ })).toBeInTheDocument();
    answer(holdingsFixture);
    await loaded();
    expect(screen.getByRole('radiogroup', { name: 'class' })).toBe(filter);
    expect(within(filter).getByRole('radio', { name: /^objects\s*14$/ })).toBeInTheDocument();
  });

  it('requests live-cell class filtering before pagination and resets its cursor', async () => {
    // Re-pinned on purpose (lane c2-chain): the unfiltered page carries its total, as the API
    // sends it (/addresses/{addr}/cells: "an exact total of all matching current cells"), since
    // the library's pager counts the page of it (`1–14 of 40 · 20 a page`, `next →`; it was a
    // Next button with no count); the class filter is the library's chips, a radio group "class"
    // (was a group of pressed buttons "Filter live cells").
    // Re-pinned on purpose (lane cd-fixes-controls, item 1): the page agrees with itself
    // (pagedRead): a first page of 20 live cells of the 40 the address holds, with more after
    // it; it served the 14 object cells and said more followed, a short page that is no first
    // page and hid the crash below (its range read 1–14 of 40)
    const firstCells = liveCellsPage(20);
    vi.mocked(api.getAddressCells).mockImplementation(async (_a, p) =>
      p?.class === 'identity'
        ? pagedRead(pageFixture(addressIdentitiesFixture))
        : p?.class === 'dao'
          ? pagedRead(pageFixture(addressDaoCellsFixture))
          : p?.cursor
            ? // the second and last page: the address's other 20 cells
              pagedRead(pageFixture(liveCellsPage(20, 20), holdingsFixture.liveCellsCount), 20)
            : pagedRead({
                ...pageFixture(firstCells, holdingsFixture.liveCellsCount),
                hasMore: true,
                nextCursor: 'next-cell',
              })
    );
    render(<AddressDetailPage />);
    await loaded();
    await tab('Live cells');
    const history = screen.getByRole('region', { name: 'History' });
    const pages = await within(history).findByRole('navigation', { name: 'live cells' });
    expect(pages).toHaveTextContent(new RegExp(`^1–20 of ${holdingsFixture.liveCellsCount} · 20`));
    fireEvent.click(within(pages).getByRole('button', { name: 'next →' }));
    await waitFor(() =>
      expect(api.getAddressCells).toHaveBeenCalledWith(addressHash, {
        limit: 20,
        cursor: 'next-cell',
        class: undefined,
      })
    );
    fireEvent.click(
      within(screen.getByRole('radiogroup', { name: 'class' })).getByRole('radio', {
        name: /^identities/,
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
        ? // a full page of 20 of the 1,682 (lane cd-fixes-controls, review-p-tests SHOULD-5: the address's few
          // activities said more followed on a page of twenty)
          pagedRead({
            ...pageFixture(activitiesPage(20), 1682),
            hasMore: true,
            nextCursor: 'next',
          })
        : // a filtered page with an older one after it, so its pager and its range are drawn: a
          // full page of one activity of another kind, as the API reads one past the page to say
          // more follow (it was an empty page that said more followed, which the API cannot send)
          pagedRead({
            ...pageFixture(activitiesPage(1, 40)),
            limit: 1,
            hasMore: true,
            nextCursor: 'next',
          })
    );
    render(<AddressDetailPage />);
    await loaded();
    // Re-pinned on purpose (lane c2-chain): the views are the library's tab row, the count after
    // the lowercase name, read as a word apart ("activities 1,682"; was a pressed button
    // "Activities1,682"); the activities are a list in time, so the library's pager counts the
    // pages of the unfiltered total (`page 1 of 85`; was "Showing 1–1 of 1,682 activities").
    const activitiesTab = () => screen.getByRole('tab', { name: /^activities/ });
    await waitFor(() => expect(activitiesTab()).toHaveTextContent(/^activities 1,682$/));
    expect(activitiesTab()).not.toHaveTextContent('1,021');
    const pages = await screen.findByRole('navigation', { name: 'activities' });
    expect(pages).toHaveTextContent(/^newest first/);
    expect(within(pages).getByText('page 1 of 85')).toBeInTheDocument();
    // A filtered page declares no total; the tab keeps the address's count, the pager has none.
    fireEvent.click(screen.getByRole('radio', { name: /^dao/ }));
    // Restored (review of main..fix/readability, test finding 3): readability B5 drew no pager for
    // one page, and the re-pin of this test dropped the range check with it; the filtered mock now
    // has an older page, so the pager is drawn and must carry no unfiltered total ("of 85").
    // Re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-5): the filtered page
    // holds one activity, a full page of one with more after it; it was an empty page that said
    // more followed, which the API cannot send (it reads one row past a page to say more follow),
    // and the page said "No DAO activities on this page." of it. The filtered page's own pager is
    // waited for: the unfiltered page stands, kept, while the filtered one loads.
    const filtered = await waitFor(() => {
      const nav = screen.getByRole('navigation', { name: 'activities' });
      expect(within(nav).getByText('page 1')).toBeInTheDocument();
      return nav;
    });
    expect(filtered).not.toHaveTextContent(/ of /);
    expect(activitiesTab()).toHaveTextContent(/^activities 1,682$/);
    await tab('Transactions');
    expect(activitiesTab()).toHaveTextContent(/^activities 1,682$/);
  });
  // PA-17 (UI review 2026-11-14): "Activities 3  Transactions 3  Live cells 3" wrapped at 390,
  // leaving "Live cells 3" alone on a second line like a heading. The labels alone are 32
  // characters: 326.4 px at 17 px and the two 24 px gaps, 374.4, more than the 326 px at 360. One
  // type step down below md (13 px labels, 11 px counts, a 6 px label–count gap, 16 px between
  // tabs) they take 249.6 + 18 + 32 = 299.6 px, and three one-digit counts 19.8: 319.4 at 360.
  it('sets the three history tabs one type step smaller below md, one row at 360 (PA-17)', async () => {
    // the activity total the Activities tab counts (the shared fixture's page declares none)
    vi.mocked(api.getAddressActivities).mockResolvedValue(pageFixture(addressActivitiesFixture, 3));
    render(<AddressDetailPage />);
    await loaded();
    // Re-pinned on purpose (lane c2-chain): the history opens on the library's tab row, the
    // section's command line (the prompt square, the board's names lowercase: activities,
    // transactions, live cells), whose phone form is the component's own: 17 px names from 640
    // and 14 px below it, 12 px counts in the quiet ink, the counts leaving the row by measurement
    // where it is wider than its line (the board's 356.4 px with three one-digit counts: they stay
    // at 390, leave at 360; lib/control-row-fit.ts). It was a group of buttons a step down below md.
    const views = screen.getByRole('tablist', { name: 'history' });
    expect(views.querySelector('.section-mark')).not.toBeNull();
    const tabs = within(views).getAllByRole('tab');
    await waitFor(() =>
      expect(tabs.map((t) => t.textContent)).toEqual([
        'activities 3',
        'transactions 1,021',
        'live cells 40',
      ])
    );
    for (const t of tabs) {
      expect(t).toHaveClass('text-body', 'sm:text-section', 'lowercase');
      expect(within(t).getByText(/^[\d,]+$/)).toHaveClass('text-meta', 'text-text-dim');
    }
    expect(screen.getByRole('heading', { level: 2, name: 'History' })).toHaveClass('sr-only');
  });
  // F2 blocks (BA-35, R2b-a filter=script): one "Script" chip, as the Address board.
  it('uses one script-call filter and resets the activity cursor', async () => {
    // re-pinned (lane cd-fixes-controls, item 1): the pages agree with themselves (pagedRead): a
    // full first page with an older one after it, then that older and last page (it served the
    // same short page with more after it at every cursor, whose next cursor repeated the one it
    // was asked with: an API fault the pager now refuses)
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.filter !== 'all'
        ? pagedRead(pageFixture([]))
        : p.cursor
          ? pagedRead(pageFixture(activitiesPage(5, 20)), 20)
          : pagedRead({ ...pageFixture(activitiesPage(20)), hasMore: true, nextCursor: 'next-act' })
    );
    render(<AddressDetailPage />);
    await loaded();
    const history = screen.getByRole('region', { name: 'History' });
    // re-pinned (lane c2-chain): the activities, a list in time, page on with `older →` (was
    // Next), and the kinds are the library's chips, a radio group "kind" whose words are lowercase
    // (were pressed buttons "Filter activity", "All", "CKB", …)
    fireEvent.click(await within(history).findByRole('button', { name: 'older →' }));
    await waitFor(() =>
      expect(api.getAddressActivities).toHaveBeenLastCalledWith(addressHash, {
        limit: 20,
        cursor: 'next-act',
        filter: 'all',
      })
    );
    // C14: each chip carries its count after its label.
    const chips = within(screen.getByRole('radiogroup', { name: 'kind' }))
      .getAllByRole('radio')
      .map((b) => b.textContent!.replace(/ ?[\d,]+\+?$/, ''));
    expect(chips).toEqual(['all', 'ckb', 'token', 'object', 'identity', 'dao', 'script', 'rgb++']);
    fireEvent.click(screen.getByRole('radio', { name: /^script/ }));
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
    // re-pinned (lane c2-chain): the kind is a chip of the library's radio group, the view a tab
    fireEvent.click(screen.getByRole('radio', { name: /^token/ }));
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
    expect(screen.getByRole('tab', { name: /^activities/ })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    expect(screen.getByRole('radio', { name: /^all/ })).toHaveAttribute('aria-checked', 'true');
  });
  it.each(['dotbit', 'did_ckb', 'bit_cell', 'dotcell'] as const)(
    'links %s identity deltas through the shared activity row',
    async (standard) => {
      vi.mocked(api.getAddressActivities).mockResolvedValue(
        pageFixture([
          {
            ...addressActivitiesFixture[0],
            // the route states each item's movement in the whole transaction (the third read's Lb SHOULD 2)
            itemDeltas: [
              {
                kind: 'identity',
                standard,
                identityId: fixtureHash(77),
                delta: 1,
                movement: 'transfer',
              },
            ],
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
          // a transfer in, as the route states it (the third read's Lb SHOULD 2): its direction
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash: fixtureHash(300),
              delta: '1',
              decimals: 8,
              movement: 'transfer',
            },
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
            {
              kind: 'token',
              typeScriptHash: fixtureHash(300),
              delta: '123456789',
              decimals: 8,
              movement: 'transfer',
            },
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
    // a feed line's token change in the summary form, its exact value in its title (readability
    // R5, re-pinned on purpose: the text was every decimal)
    expect((await screen.findAllByTitle('+1.23456789')).length).toBeGreaterThan(0);
    expect(screen.getAllByTitle('+1.23456789')[0]).toHaveTextContent(/^\+1\.23$/);
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
    // the balance figure in the summary form, exact in its title (readability R5, re-pinned on
    // purpose: was every decimal)
    expect(screen.getByLabelText('Balance')).toHaveTextContent('balance9,651,178 CKB');
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
  // Renamed and re-pinned on purpose (lane c2-chain; was "counts only committed activity rows in
  // the page range", which read "Showing 1–1 activities"): the activities are a list in time, and
  // the library's pager states a list in time by its order and its page alone, no count of rows a
  // pool row could be added to.
  it('counts no unconfirmed row in the pager: a list in time states its order and its page', async () => {
    // Re-pinned on purpose (readability B5): one page draws no pager, so the fixture has an older
    // page.
    // a full page of 20 committed rows under the pool's row (lane cd-fixes-controls, SHOULD-5:
    // the address's few activities said more followed on a page of twenty); the filter chips'
    // one-row pages are empty
    vi.mocked(api.getAddressActivities).mockImplementation(async (_l, p) =>
      p?.limit === 1
        ? pagedRead(pageFixture([]))
        : pagedRead({
            ...pageFixture([poolActivity(), ...activitiesPage(20)]),
            hasMore: true,
            nextCursor: 'next',
            pool,
          })
    );
    render(<AddressDetailPage />);
    await loaded();
    const pages = await screen.findByRole('navigation', { name: 'activities' });
    expect(pages).toHaveTextContent(/^newest first← newerpage 1older →$/);
  });
  // C11: the address lists page through the one pager; its pages follow the page size the list
  // asked for (twenty on every width), where the old pager counted fifty a page everywhere.
  // Re-pinned on purpose (lane c2-chain): the activities are a list in time, so the library's
  // pager writes `newest first`, `page 1 of 3` and `older →` (was "Showing 1–20 of 45 activities
  // · 20 per page", "Page 1 of 3" and a Next button), in a nav named "activities" (was
  // "activities pages").
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
          ? pagedRead({ ...pageFixture(rows(20), 45), hasMore: true, nextCursor: 'third' })
          : pagedRead({ ...pageFixture(rows(0), 45), hasMore: true, nextCursor: 'second' })
    );
    render(<AddressDetailPage />);
    await loaded();
    const pages = await screen.findByRole('navigation', { name: 'activities' });
    expect(within(pages).getByText('newest first')).toBeVisible();
    expect(within(pages).getByText('page 1 of 3')).toBeVisible();
    fireEvent.click(within(pages).getByRole('button', { name: 'older →' }));
    expect(await within(pages).findByText('page 2 of 3')).toBeVisible();
    expect(within(pages).getByRole('button', { name: '← newer' })).toBeVisible();
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
    // two pages that agree with themselves (lane cd-fixes-controls, SHOULD-5: the address's few
    // activities said more followed on a page of twenty, at every cursor)
    vi.mocked(api.getAddressActivities).mockImplementation(async (_l, p) =>
      p?.limit === 1
        ? pagedRead(pageFixture([]))
        : p?.cursor
          ? pagedRead(pageFixture(activitiesPage(5, 20)), 20)
          : pagedRead({ ...pageFixture(activitiesPage(20)), hasMore: true, nextCursor: 'next' })
    );
    render(<AddressDetailPage />);
    await loaded();
    // re-pinned (lane c2-chain): `older →` of the library's pager (was Next), the dao chip a radio
    fireEvent.click(
      await within(screen.getByRole('region', { name: 'History' })).findByRole('button', {
        name: 'older →',
      })
    );
    expect(screen.getByLabelText('Unconfirmed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio', { name: /^dao/ }));
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
    // re-pinned on purpose (lane D4): the phone row writes the status and the wait as its first
    // line's figure, one run of text (it was the table's badge over the wait, two elements)
    expect(mobile.querySelector('[data-line="identity"] [data-part="right"]')).toHaveTextContent(
      'Proposed · in pool for 2m'
    );
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
    // the balance figure in the summary form, exact in its title (readability R5, re-pinned on
    // purpose: was every decimal)
    expect(screen.getByLabelText('Balance')).toHaveTextContent('balance9,651,178 CKB');
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
    // re-pinned (A4 round 2): the counts are units of their own in the line
    expect(
      await screen.findByText(fullText('1 deposit · 1 withdrawal request'))
    ).toBeInTheDocument();
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
    // re-pinned (lane c2-chain): the library's CopyButton keeps its name and answers in words
    // beside it, `copied`, heard as a status (it renamed itself "Address copied")
    const copy = screen.getByRole('button', { name: 'copy address' });
    // lane c2-finish, rulings round 1: the copy's answer stands out of the line's flow under
    // its box, and nothing above the copy clips it
    expectAnswerUnclipped(copy);
    fireEvent.click(copy);
    await waitFor(() =>
      expect(within(copy.parentElement!).getByRole('status')).toHaveTextContent(/^copied$/)
    );
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
    // re-pinned on purpose (lane P1, third round; the lead's ruling on MUST 3 and SHOULD 3): the
    // route's three parts, each true by its name (they were occupied, spendable and in the DAO)
    expect(parts.map((p) => p.dataset.part)).toEqual(['in Nervos DAO cells', 'occupied', 'free']);
    for (const part of parts) expect(part).toHaveClass('min-w-[3px]');
    expect(parts[0].parentElement).toHaveClass('h-[18px]', 'gap-0.5');
    expect(balance).toHaveTextContent('9,590,000 CKB in 5 deposits');
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
    // lane P1, the ink and size roles: the type hash is an id, at the row's 13 px (it was 12)
    for (const row of withHash)
      expect(typeStepOf(row.querySelector('[data-type-hash]')!.firstElementChild!)).toBe('small');
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
    // B4 (re-pinned): the second transaction has another party, so the two rows do not say the
    // same thing and are not folded into one run (they were two transfers of this address alone)
    // Re-pinned on purpose (Gate B's rule, lists lane P3: a run is what was done and the script
    // code, not the parties): the second transaction calls a type script, so the two rows do not
    // fold (another party alone no longer keeps them apart)
    const second = activityRead({
      ...addressActivitiesFixture[0],
      txHash: fixtureHash(801),
      ckbDelta: '-5',
      typeCalls: [
        {
          typeCodeHash: fixtureHash(803),
          typeHashType: 'type',
          typeArgs: '0x',
          scriptHash: fixtureHash(804),
          scriptName: 'Some Type',
        },
      ],
      // re-pinned on purpose (lane feed, ruling 14 of gate P): the other party by its short id, as
      // the route names every party (it was sent in full as one of the row's `participants`)
      participantCount: 2,
      partyIds: [
        [addressFixture.lockScriptHash.slice(0, 42), 0],
        [fixtureHash(802).slice(0, 42), 0],
      ],
      feePayer: addressFixture.lockScriptHash,
    });
    const page = pageFixture([...addressActivitiesFixture, second]);
    // C14 asks each filter chip for a one-row page; answer it with one row and hasMore, as the
    // API would, so the feed's two rows do not reach the chip's one-row invariant. Lane
    // cd-fixes-controls (review-fix-tests SHOULD-3): as the API would means a full page of one at
    // the size asked, naming the cursor after it (it was one row at 20 a page that said more
    // followed and named no cursor, which the checker refuses)
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.limit === 1
        ? pagedRead({
            ...page,
            data: page.data.slice(0, 1),
            limit: 1,
            hasMore: true,
            nextCursor: 'chip-1',
          })
        : page
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
    // re-pinned (lane P1 follow-up): the fixture's lock is the JoyID lock its address spells (its
    // code hash was 0x0…64, which the address does not spell and the lock words now refuse)
    expect(lock).toHaveTextContent(
      'code_hash0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323 · JoyID'
    );
    // Responsive fix plan 2026-11-01 (A3): in the break-all box the lock name split "Def/ault Lock".
    expect(within(lock).getByText('JoyID')).toHaveClass('whitespace-nowrap');
    expect(within(lock).getByRole('link', { name: 'Markdown' }).getAttribute('href')).toBe(
      `/mainnet/address/${addressText}.md`
    );
    // Re-pinned on purpose (lane P1 follow-up, the link-look gate): the API's JSON is a way out of
    // the site, an ExternalLink to the API's absolute URL ("JSON ↗", a new tab), not a raw <a>
    const json = within(lock).getByRole('link', { name: /^JSON\s↗$/ });
    expect(json.getAttribute('href')).toMatch(
      new RegExp(`^https?://[^/]+/api/mainnet/v1/addresses/${addressText}$`)
    );
    expect(json).toHaveAttribute('target', '_blank');
    expect(json).toHaveAttribute('rel', 'noreferrer');
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
    expect(identities).toHaveTextContent(/1 cell · 227\.99 CKB held/);
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

  // Renamed and re-pinned on purpose (lane c2-chain; was "puts the activity filters on the tab
  // row, as the board", a row of the view buttons and the chips side by side): the controls board
  // the owner passed (gate D0′) puts the address's kind chips under the tab row, the first line of
  // the tab's panel, 14 px down; the row itself is the tab list alone.
  it('puts the activity filters under the tab row, in its panel, as the controls board', async () => {
    render(<AddressDetailPage />);
    await loaded();
    const tabs = screen.getByRole('tablist', { name: 'history' });
    const chips = screen.getByRole('radiogroup', { name: 'kind' });
    expect(tabs).not.toContainElement(chips);
    expect(screen.getByRole('tabpanel')).toContainElement(chips);
    expectBefore(tabs, chips);
    expect(chips).toHaveClass('mt-3.5');
  });

  it('writes the class headers as the board: "9 kinds in 15 cells", "14 in 7 collections"', async () => {
    render(<AddressDetailPage />);
    expect(await section(/Tokens$/)).toHaveTextContent(/9 kinds in 15 cells · 21,507 CKB held/);
    expect(await section(/Objects$/)).toHaveTextContent(/14 in 7 collections · 4,093 CKB held/);
  });
});

// Consistency round (Address.dc.html): the h1 is the short address (C13); Holdings collapses the
// classes it does not hold into one line, leads with the composition block and lists one row per
// collection that opens in place to its first ten objects (C14); every filter chip has a count.
describe('Address · consistency round (C13, C14)', () => {
  it('names the page by its short address, the full address and its copy button beneath (C13)', async () => {
    render(<AddressDetailPage />);
    await loaded();
    // R9 (re-pinned): the head size of an address is `ckb1…` and its last 8 characters
    const h1 = screen.getByRole('heading', {
      level: 1,
      name: `${addressText.slice(0, 4)}…${addressText.slice(-8)}`,
    });
    expect(h1).toHaveClass('text-entity');
    const header = h1.closest('header')!;
    expect(header).toHaveTextContent(addressText);
    // re-pinned (lane c2-chain): the library's CopyButton, named in the board's lowercase
    expect(within(header).getByRole('button', { name: 'copy address' })).toBeInTheDocument();
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
  // Re-pinned on purpose (lane D2; renamed, it was "leads with the balance figure and its
  // partition, then the four metrics, then Holdings"): the four metrics are the head's status
  // strip, under the id line and before the balance, said once; tier 1 leads with the balance and
  // its partition, then Holdings.
  it('leads with the balance figure and its partition, then Holdings; the four facts are the strip above them', async () => {
    const { container } = render(<AddressDetailPage />);
    await loaded();
    const tier1 = container.querySelector('[data-tier="1"]')!;
    expect(tier1.querySelector('section')).toHaveAttribute('aria-label', 'Balance');
    const balance = screen.getByRole('region', { name: 'Balance' });
    const strip = screen.getByRole('group', { name: 'Address status' });
    expect(screen.queryByRole('region', { name: 'Metrics' })).toBeNull();
    const holdings = screen.getByRole('heading', { name: 'Holdings 40 cells' });
    expect(strip.compareDocumentPosition(balance) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      balance.compareDocumentPosition(holdings) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // the summary form (R5, re-pinned on purpose: was "9,651,178.35338057 CKB")
    expect(balance.querySelector('[data-role="balance-figure"]')).toHaveTextContent(
      /^9,651,178 CKB$/
    );
    expect(balance.querySelector('[data-role="balance-figure"] [data-ckb-amount]')).toHaveAttribute(
      'title',
      '9,651,178.35338057 CKB'
    );
    expect([...strip.querySelectorAll('dt')].map((d) => d.textContent)).toEqual([
      'lock',
      'live cells',
      'transactions',
      'holdings',
    ]);
  });
  // Lane P (exact or none): a filter chip shows a count only when its one-row page proves it — 0
  // or 1 — and no number otherwise; "All" keeps the address's total; an empty filter is dimmed.
  it('counts a filter chip from its one-row page only when the count is exact; an empty filter is dimmed 0', async () => {
    // a chip's one-row page as the API sends it: at the size asked (1), naming the cursor after
    // it where more follow (lane cd-fixes-controls, review-fix-tests SHOULD-3: they were pages of
    // 20, and "ckb"'s said more followed and named no cursor)
    const chipPage = (rows: typeof addressActivitiesFixture, more: boolean) =>
      pagedRead({ data: rows, limit: 1, hasMore: more, nextCursor: more ? 'chip-1' : null });
    vi.mocked(api.getAddressActivities).mockImplementation(async (_a, p) =>
      p?.limit === 1 && p.filter && p.filter !== 'all'
        ? p.filter === 'token'
          ? chipPage([addressActivitiesFixture[0]], false)
          : p.filter === 'ckb'
            ? chipPage([addressActivitiesFixture[0]], true)
            : chipPage([], false)
        : // the unfiltered page states the address's activity count (contracts W5-1)
          pageFixture(addressActivitiesFixture, addressActivitiesFixture.length)
    );
    render(<AddressDetailPage />);
    await loaded();
    // Re-pinned on purpose (lane c2-chain): the kinds are the library's chips, a radio group
    // "kind" whose words are lowercase and whose count is a word apart (were pressed buttons
    // "Filter activity", "DAO 0"); the four classes lead with their 7 px class mark, as the
    // controls board's address in place draws them.
    const chips = screen.getByRole('radiogroup', { name: 'kind' });
    await waitFor(() =>
      expect(within(chips).getByRole('radio', { name: /^dao/ })).toHaveTextContent('dao 0')
    );
    expect(within(chips).getByRole('radio', { name: /^all/ })).toHaveTextContent(
      `all ${addressActivitiesFixture.length}`
    );
    expect(within(chips).getByRole('radio', { name: /^token/ })).toHaveTextContent(/^token 1$/);
    // More than the one row asked for: the API counts no filtered total, so the chip shows no
    // number (never "2+"); what the page proved stays in the chip's title.
    const ckb = within(chips).getByRole('radio', { name: /^ckb/ });
    expect(ckb).toHaveTextContent(/^ckb$/);
    expect(ckb).toHaveAttribute(
      'title',
      'more than one; the API keeps no count of filtered activity'
    );
    expect(chips).not.toHaveTextContent('2+');
    for (const chip of within(chips).getAllByRole('radio'))
      if (chip !== ckb) expect(chip).toHaveTextContent(/\d$/);
    for (const kind of ['token', 'object', 'identity', 'dao'])
      expect(
        within(chips)
          .getByRole('radio', { name: new RegExp(`^${kind}`) })
          .querySelector('[data-mark]')
      ).not.toBeNull();
    expect(ckb.querySelector('[data-mark]')).toBeNull();
    // R4 (UI review 2026-11-14, re-pinned on purpose): an empty filter's "0" is a datum, so the
    // chip is the dim ink of every other chip, never the ghost.
    const dao = within(chips).getByRole('radio', { name: /^dao/ });
    expect(dao).toHaveTextContent(/^dao 0$/);
    expect(dao).toHaveClass('text-text-dim');
    expect(dao).not.toHaveClass('text-text-ghost');
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
    daoCellsCapacity: '0',
    otherCellsOccupiedCapacity: '388700000000',
    otherCellsFreeCapacity: '3455575078856',
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
      // re-pinned on purpose (lane P1, the plan: the bar says its unit; review chain-b
      // address-miner: "102 / 122" with no unit read as CKB): bytes, "3,761 B"
      // lane exactness, part B2: 3,761 of 3,887 B is 96.758…%, 96.75% (half up 96.76%); 126 3.2415…%, 3.24%
    ).toEqual(['Objects3,761\u00a0B96.75%', 'Plain CKB126\u00a0B3.24%']);
    const rest = within(legend).getByText('nothing in DAO, tokens, identities or other scripts');
    // re-pinned on purpose (lane P1, the ink and size roles: a sentence outside a <p> takes the
    // reading ink; it was the quiet ink)
    expect(inkOf(rest)).toBe(READING);
    expect(rest).toHaveAttribute(
      'title',
      'Nervos DAO 0 B · Tokens 0 B · Identities 0 B · Other scripts 0 B'
    );
    // the bar's own line says it measures bytes, and its total is bytes
    const space = legend.closest('div')!.parentElement!;
    expect(within(space).getByText('On-chain space they take, in bytes')).toBeInTheDocument();
    expect(space).toHaveTextContent('3,887 B occupied in 16 cells');
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

// Lane P1, the ink and size roles round (BRIEF-ink-size-roles, items 1 to 3): a value, a table
// cell's content and a sentence read in the reading ink at 13 px; a label, a unit, a column head, a
// hint and an eyebrow stay quiet. These lines stood in the quiet ink, some at the meta step.
describe('the ink and size roles of the address page (lane P1)', () => {
  it('says an unavailable lock script and an empty history in the reading ink', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({ ...addressFixture, lockScript: undefined });
    vi.mocked(api.getAddressActivities).mockResolvedValue(pageFixture([]));
    render(<AddressDetailPage />);
    expect(inkOf(await screen.findByText('Script unavailable'))).toBe(READING);
    expect(inkOf(await screen.findByText('No activities.'))).toBe(READING);
  });

  it('reads a transaction’s date and notes in its table cells in the reading ink at 13 px', async () => {
    const named = { ...addressTransactionsFixture[0], txType: 'named' as const };
    const pooled = {
      ...addressTransactionsFixture[0],
      txHash: fixtureHash(801),
      blockNumber: null,
      timestamp: null,
      poolStatus: 'pending' as const,
      timeAddedToPool: '1771632000000',
      interpretation: { status: 'partial' as const, reasons: [{ code: 'unresolved_input' }] },
    };
    vi.mocked(api.getAddressTransactions).mockResolvedValue(pageFixture([pooled, named], 2));
    render(<AddressDetailPage />);
    await tab('Transactions');
    const table = await screen.findByRole('table', { name: /transactions/i });
    const note = await within(table).findByText('Named participant');
    expect(inkOf(note)).toBe(READING);
    expect(typeStepOf(note)).toBe('small');
    expect(inkOf(within(table).getByText(formatUtc8Date('1771632000000')))).toBe(READING);
    expect(typeStepOf(within(table).getByText(/^partial: /))).toBe('small');
  });

  it('says a phone row’s named participant in the reading ink, its pool note at the line’s size', async () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    const named = { ...addressTransactionsFixture[0], txType: 'named' as const };
    const pooled = {
      ...addressTransactionsFixture[0],
      txHash: fixtureHash(801),
      blockNumber: null,
      timestamp: null,
      poolStatus: 'pending' as const,
      timeAddedToPool: '1771632000000',
      interpretation: { status: 'partial' as const, reasons: [{ code: 'unresolved_input' }] },
    };
    vi.mocked(api.getAddressTransactions).mockResolvedValue(pageFixture([pooled, named], 2));
    render(<AddressDetailPage />);
    await tab('Transactions');
    const note = await screen.findByText('Named participant');
    expect(note.closest('table')).toBeNull();
    expect(inkOf(note)).toBe(READING);
    // the touched line is a body line, 13 px: the note takes it (it set the meta step)
    expect(typeStepOf(screen.getByText(/^partial: /))).toBe('small');
  });

  it('states a holding’s summary, the collections’ sentence and an unread composition in the reading ink', async () => {
    vi.mocked(api.getAddressCells).mockImplementation(async (_addr, p) =>
      pageFixture(
        addressObjectsFixture
          .filter((c) => c.display!.collectionId === p?.collection)
          .map((c) => ({ ...c, display: { ...c.display!, compositionTier: null } })),
        6
      )
    );
    render(<AddressDetailPage />);
    await loaded();
    const dao = await waitFor(() => {
      const h = document.querySelector('[data-holding-class="dao"] h3');
      expect(h).toHaveTextContent('Nervos DAO deposits');
      return h!;
    });
    expect(inkOf(dao.nextElementSibling!)).toBe(READING);
    expect(inkOf(await screen.findByText(/^Open a collection to list its objects here/))).toBe(
      READING
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open 拯救民主' }));
    const list = await screen.findByRole('table', { name: '拯救民主 objects held here' });
    const unread = within(list).getAllByText('Composition unavailable');
    for (const el of unread) expect(inkOf(el)).toBe(READING);
  });
});
