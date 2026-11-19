// F2 group `objects` (round 2): the Inventory boards — Objects lead with composition (IV-09),
// CKB held drawn as a bar in the class colour in all three views (IV-02), the token chips and
// names (IV-03, IV-04), occupied as whole CKB (IV-06), instant search and the board's "sorted by"
// line (IV-07), crystal / copper thumbnail frames (IV-10), "m-NFT" (IV-11) and a quiet identities
// view (IV-12).
import { afterEach, describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { containerContentPx, contentPx } from '@/__tests__/utils/tailwind-box';
import TokensPage from '@/app/inventory/tokens/page';
import ObjectsPage from '@/app/inventory/objects/page';
import IdentitiesPage from '@/app/inventory/identities/page';
import { api, type Asset, type InventorySummary } from '@/lib/api';

vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(window.location.search),
  usePathname: () => window.location.pathname,
  useRouter: () => ({
    replace: (href: string) => {
      const url = new URL(href, 'http://localhost');
      window.history.replaceState(null, '', `${url.pathname}${url.search}`);
    },
  }),
}));
vi.mock('@/lib/api', () => ({
  api: { getAssets: vi.fn(), getInventorySummary: vi.fn() },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => null }));

const summary: InventorySummary = {
  source: 'canonical_asset_indexes',
  anchor: { blockNumber: 20876536, blockHash: '0x' + 'aa'.repeat(32) },
  counts: { token: 727, object: 5653, identity: 4 },
  objectStandards: [
    { standard: 'm-nft', collectionsCount: 5209, liveCount: 45994 },
    { standard: 'spore', collectionsCount: 444, liveCount: 37269 },
  ],
  objectComposition: [
    { tier: 'btc_ckb', collectionsCount: 35, liveCount: 22084 },
    { tier: 'centralized_mixture', collectionsCount: 5258, liveCount: 50000 },
    { tier: 'decentralized_mixture', collectionsCount: 8, liveCount: 100 },
    { tier: 'decode_pending', collectionsCount: 0, liveCount: 0 },
    { tier: 'pure_ckb', collectionsCount: 322, liveCount: 4782 },
    { tier: 'unknown', collectionsCount: 30, liveCount: 6297 },
  ],
  fullyOnchainObjectsCount: 26880,
  tokenStandards: [
    { standard: 'sudt', count: 94 },
    { standard: 'xudt', count: 629 },
    { standard: 'xudt_compatible', count: 4 },
  ],
  pendingItemsCount: 0,
};
const base = {
  liveCount: null,
  activitiesCount: null,
  compositionTier: null,
  onchainRatio: null,
  onchainCount: null,
  iconUrl: null,
  published: false,
  famous: false,
  tags: null,
  holdersCount: 10,
  transfersCount: 20,
  transfers24h: 0,
  decimals: 8,
  totalSupply: '100000000',
  contentType: null,
  contentSize: null,
  clusterId: null,
  clusterName: null,
  hMultiplier: '1.50',
};
const page = (data: Asset[]) => ({
  data,
  total: data.length,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  anchor: { blockNumber: 21016902, blockHash: `0x${'ab'.repeat(32)}` },
});
const tokens = page([
  {
    ...base,
    id: '0x' + '11'.repeat(32),
    assetType: 'token',
    standard: 'xudt',
    name: 'RGB++ Protocol',
    symbol: 'RGB++',
    ownedCapacity: '3067010300000000',
    ownedKnowledge: '1000000000',
  },
  {
    ...base,
    id: '0x' + '22'.repeat(32),
    assetType: 'token',
    standard: 'sudt',
    name: null,
    symbol: 'SEAL',
    ownedCapacity: '766752575000000',
    ownedKnowledge: '183800000000',
  },
] as Asset[]);
const objects = page([
  {
    ...base,
    id: '0x' + '33'.repeat(32),
    assetType: 'object',
    standard: 'spore',
    name: 'Nervape',
    symbol: null,
    liveCount: 2777,
    totalSupply: '2777',
    activitiesCount: 5137,
    compositionTier: 'btc_ckb',
    ownedCapacity: '98373077776763',
    ownedKnowledge: '71322600000000',
  },
  {
    ...base,
    id: '0x' + '44'.repeat(32),
    assetType: 'object',
    standard: 'm-nft',
    name: 'Santa',
    symbol: null,
    liveCount: 2408,
    totalSupply: '2408',
    activitiesCount: 3212,
    compositionTier: 'centralized_mixture',
    ownedCapacity: '32285294386739',
    ownedKnowledge: '32053400000000',
  },
] as Asset[]);
const identities = page([
  {
    ...base,
    id: 'dotbit',
    assetType: 'identity',
    standard: 'dotbit',
    name: '.bit',
    symbol: null,
    liveCount: 6094,
    totalSupply: '250043',
    ownedCapacity: '131000000000000',
    ownedKnowledge: '126000000000000',
  },
] as Asset[]);

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getInventorySummary).mockResolvedValue(summary);
});

// Responsive fix plan 2026-11-01 (A12a, report B#25): at 390 the three class tabs (~414 px in one
// line) wrapped "Identities 4" onto a line of its own. Re-pinned on purpose (controls lane C2):
// the classes are the controls board's tab row, the page's command line, one line at every width
// by the one fit function (lib/control-row-fit.ts): at a 360 px window its names and counts are
// 324 px in the 328 px line, so the counts stay (the board's caption). It stacked each count under
// its label below sm.
describe('Inventory class tabs across widths', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/inventory/objects');
    vi.mocked(api.getAssets).mockResolvedValue(objects);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('keeps the three classes and their counts on one line of a 360 px window (324 px in 328)', async () => {
    atViewportWidth(360);
    stubChartWidth(containerContentPx(360));
    render(<ObjectsPage />);
    const list = await screen.findByRole('tablist', { name: 'Inventory classes' });
    await waitFor(() => expect(list).toHaveTextContent('5,653'));
    expect(
      within(list)
        .getAllByRole('tab')
        .map((t) => t.textContent)
    ).toEqual(['tokens 727', 'objects 5,653', 'identities 4']);
    expect(within(list).getByRole('tab', { name: 'objects 5,653' })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    expect(Math.round(contentPx(list, 360) * 100) / 100).toBe(324);
  });
});

describe('Objects view: composition first (IV-09, IV-10, IV-11)', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/inventory/objects');
    vi.mocked(api.getAssets).mockResolvedValue(objects);
  });
  it('states how many collections are fully on-chain, with a share bar and counted chips', async () => {
    render(<ObjectsPage />);
    const comp = await screen.findByRole('region', { name: 'Object composition' });
    expect(comp).toHaveTextContent(
      '357 of 5,653 collections are fully on-chain · so are 26,880 live objects'
    );
    // Responsive fix plan 2026-11-01 (A12b, report B#27): the sentence no longer stops at "are".
    expect(comp).not.toHaveTextContent(/live objects are\b/);
    expect(comp).toHaveTextContent('444 Spore clusters · 5,209 m-NFT classes');
    expect(
      within(comp).getByRole('img', {
        // C8: the shared composition block names its set bar by the set.
        name: /Composition of the 5,653 collections: BTC\+CKB 35, Pure CKB 322/,
      })
    ).toBeInTheDocument();
    const chips = within(comp).getByRole('group', { name: 'Filter by composition' });
    expect(within(chips).getByRole('button', { name: 'All 5,653' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
    const btc = within(chips).getByRole('button', { name: /BTC\+CKB 35/ });
    expect(btc.querySelector('[data-composition-mark="btc_ckb"]')).not.toBeNull();
    // C8: every tier is a chip with its count; a tier no collection has stays, dimmed and off.
    expect(within(chips).getAllByRole('button')).toHaveLength(6);
    for (const chip of within(chips).getAllByRole('button')) expect(chip).toHaveTextContent(/\d/);
    fireEvent.click(btc);
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ compositionTier: 'btc_ckb' })
      )
    );
    expect(screen.queryByRole('group', { name: 'Standard filters' })).not.toBeInTheDocument();
  });
  // Re-pinned on purpose (readability B5, plan §7.6): the violet CKB held bar left the list; the
  // amount, right aligned under its head, shows the magnitude.
  it('frames on-chain thumbnails, says m-NFT and prints CKB held with no bar', async () => {
    render(<ObjectsPage />);
    const table = await screen.findByRole('table', { name: 'Object collections' });
    expect(table.querySelector('[data-thumb-frame="btc_ckb"]')).not.toBeNull();
    expect(table.querySelector('[data-thumb-frame="centralized_mixture"]')).toBeNull();
    expect(within(table).getByText(/^m-NFT/)).toBeInTheDocument();
    expect(table.querySelectorAll('[data-segment]')).toHaveLength(0);
    // the amount in the list's summary form (lane B2, readability R5, re-pinned on purpose: it
    // was CkbTwoPlaces' [data-ckb-two-places])
    expect(table.querySelectorAll('[data-ckb-amount][data-form="summary"]')).toHaveLength(2);
  });
});

describe('Tokens view (IV-02, IV-03, IV-04, IV-06, IV-07)', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/inventory/tokens');
    vi.mocked(api.getAssets).mockResolvedValue(tokens);
  });
  it('names each token in full and offers every token standard with an "All" count', async () => {
    render(<TokensPage />);
    const table = await screen.findByRole('table', { name: 'Tokens' });
    expect(within(table).getByText('RGB++ Protocol')).toBeInTheDocument();
    // Table-rows L2-1: the symbol is a tag on the name's line; xUDT is in the row's title only.
    expect(within(table).getByText('RGB++')).toHaveAttribute('data-row-tag', 'default');
    const [, rgbpp, seal] = within(table).getAllByRole('row');
    expect(rgbpp.getAttribute('title')).toMatch(/^RGB\+\+ Protocol · RGB\+\+ · xUDT · /);
    expect(within(table).getByRole('link', { name: 'SEAL' })).toBeInTheDocument();
    expect(within(seal).getByText('sUDT')).toHaveAttribute('data-row-tag', 'default');
    // Re-pinned on purpose (controls lane C2): the controls board's filter chips, a radio group
    // named by the filter, its words lowercase (it was the aria-pressed group "Standard filters",
    // "All 727", "xUDT 629", "sUDT 94", "xUDT-compatible 4").
    const chips = await screen.findByRole('radiogroup', { name: 'standard' });
    expect(
      within(chips)
        .getAllByRole('radio')
        .map((b) => b.textContent)
    ).toEqual(['all 727', 'xudt 629', 'sudt 94', 'xudt-compatible 4']);
    expect(within(chips).getByRole('radio', { name: 'all 727' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
  });
  it('counts each standard chip from the summary, which sums to All (R-3, IV-03)', async () => {
    vi.mocked(api.getInventorySummary).mockResolvedValue({
      ...summary,
      counts: { ...summary.counts, token: 723 },
      tokenStandards: [
        { standard: 'sudt', count: 94 },
        { standard: 'xudt', count: 629 },
      ],
    });
    render(<TokensPage />);
    await screen.findByRole('table', { name: 'Tokens' });
    // Re-pinned on purpose (controls lane C2): the filter chips' radio group, lowercase words.
    const chips = await screen.findByRole('radiogroup', { name: 'standard' });
    // A standard absent from the summary has no catalog token, so it has no chip.
    await waitFor(() =>
      expect(
        within(chips)
          .getAllByRole('radio')
          .map((b) => b.textContent)
      ).toEqual(['all 723', 'xudt 629', 'sudt 94'])
    );
  });
  // Re-pinned on purpose (readability B5, plan §7.6): the azure bar on the page's scale left the
  // list; CKB held is its amount, in the list's summary form (lane B2, readability R5, re-pinned
  // on purpose: it was cut to 2 dp, "30,670,103.00", "7,667,525.75").
  it('prints CKB held as its amount with no bar, and occupied in whole CKB in the title', async () => {
    render(<TokensPage />);
    const table = await screen.findByRole('table', { name: 'Tokens' });
    // Occupied leaves the row for its title.
    expect(table.querySelectorAll('[data-segment]')).toHaveLength(0);
    expect(
      Array.from(table.querySelectorAll('[data-ckb-amount]')).map((n) => n.textContent)
    ).toEqual(['30,670,103', '7,667,525']);
    expect(within(table).queryByRole('columnheader', { name: 'Occupied' })).toBeNull();
    expect(within(table).getAllByRole('row')[2].getAttribute('title')).toContain(
      '7,667,525.75 CKB held · 1,838 CKB occupied'
    );
  });
  it('searches as you type and keeps sorting in the board’s "sorted by" line', async () => {
    render(<TokensPage />);
    await screen.findByRole('table', { name: 'Tokens' });
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search the inventory' }), {
      target: { value: 'rgb' },
    });
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ search: 'rgb' }))
    );
    // Re-pinned on purpose (controls lane C2): the controls board's select, named by the label it
    // stands after, "sorted by" (it was the aria-label "Sort inventory").
    expect(screen.getByText(/^sorted by$/)).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'sorted by' })).toHaveValue('capacity');
  });
});

describe('Identities view (IV-12, IV-02)', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/inventory/identities');
    vi.mocked(api.getAssets).mockResolvedValue(identities);
  });
  // Re-pinned on purpose (readability B5, plan §7.6): the rose bars left the list.
  it('lists the systems plainly: no standard chips, no repeated name, no CKB held bar', async () => {
    render(<IdentitiesPage />);
    const table = await screen.findByRole('table', { name: 'Identity systems' });
    const name = within(table).getByRole('link', { name: '.bit' });
    expect(name).toHaveClass('text-body');
    expect(screen.queryByRole('group', { name: 'Standard filters' })).not.toBeInTheDocument();
    expect(within(table).getAllByText('.bit')).toHaveLength(1);
    expect(
      screen.getByText('4 naming systems · every identity’s record lives in its cell on CKB')
    ).toBeInTheDocument();
    expect(table.querySelector('[data-segment]')).toBeNull();
  });
});
