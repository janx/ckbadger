// F2 group `objects` (round 2): the Inventory boards — Objects lead with composition (IV-09),
// CKB held drawn as a bar in the class colour in all three views (IV-02), the token chips and
// names (IV-03, IV-04), occupied as whole CKB (IV-06), instant search and the board's "sorted by"
// line (IV-07), crystal / copper thumbnail frames (IV-10), "m-NFT" (IV-11) and a quiet identities
// view (IV-12).
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import TokensPage from '@/app/inventory/tokens/page';
import ObjectsPage from '@/app/inventory/objects/page';
import IdentitiesPage from '@/app/inventory/identities/page';
import { api, type Asset, type InventorySummary } from '@/lib/api';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { resourceScale } from '@/components/blocks/block-visual';

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
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
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
// line) wrapped "Identities 4" onto a line of its own. Below sm each tab puts its count under its
// label (~330 px for the three), so they share one line at full type size; from sm one line each.
describe('Inventory class tabs across widths', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/inventory/objects');
    vi.mocked(api.getAssets).mockResolvedValue(objects);
  });
  it('stacks each count under its label below sm so the three tabs share one line', async () => {
    render(<ObjectsPage />);
    const nav = await screen.findByRole('navigation', { name: 'Inventory classes' });
    await waitFor(() => expect(nav).toHaveTextContent('5,653'));
    expect(nav).toHaveClass('gap-x-5', 'sm:gap-x-6');
    const tabs = within(nav).getAllByRole('link');
    expect(tabs.map((t) => t.querySelector('[data-role="tab-label"]')?.textContent)).toEqual([
      'Tokens',
      'Objects',
      'Identities',
    ]);
    for (const tab of tabs) {
      expect(tab).toHaveClass('grid', 'grid-cols-[16px_auto]', 'sm:flex');
      expect(tab.querySelector('[data-role="tab-count"]')).toHaveClass('col-start-2');
    }
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
  it('frames on-chain thumbnails, says m-NFT and draws CKB held in violet', async () => {
    render(<ObjectsPage />);
    const table = await screen.findByRole('table', { name: 'Object collections' });
    expect(table.querySelector('[data-thumb-frame="btc_ckb"]')).not.toBeNull();
    expect(table.querySelector('[data-thumb-frame="centralized_mixture"]')).toBeNull();
    expect(within(table).getByText(/^m-NFT/)).toBeInTheDocument();
    // Table-rows L2-2: an 8 px AlignedBar per row on the page's round scale.
    const scale = resourceScale([983730, 322852]);
    const bars = table.querySelectorAll('[data-segment]');
    expect(bars).toHaveLength(2);
    expect(bars[0]).toHaveStyle({
      width: `${Math.round((983730 / scale) * 120 * 100) / 100}px`,
      backgroundColor: CLASS_COLORS.objects,
    });
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
    const chips = screen.getByRole('group', { name: 'Standard filters' });
    expect(
      within(chips)
        .getAllByRole('button')
        .map((b) => b.textContent)
    ).toEqual(['All 727', 'xUDT 629', 'sUDT 94', 'xUDT-compatible 4']);
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
    const chips = screen.getByRole('group', { name: 'Standard filters' });
    // A standard absent from the summary has no catalog token, so it has no chip.
    await waitFor(() =>
      expect(
        within(chips)
          .getAllByRole('button')
          .map((b) => b.textContent)
      ).toEqual(['All 723', 'xUDT 629', 'sUDT 94'])
    );
  });
  it('draws CKB held as an azure bar on the page’s scale, and occupied in whole CKB in the title', async () => {
    render(<TokensPage />);
    const table = await screen.findByRole('table', { name: 'Tokens' });
    // Table-rows L2-1: an 8 px AlignedBar per row on one round page scale (30,670,103 and
    // 7,667,525 whole CKB); Occupied leaves the row for its title.
    const scale = resourceScale([30670103, 7667525]);
    const bars = table.querySelectorAll('[data-segment]');
    expect(bars[0]).toHaveStyle({
      width: `${Math.round((30670103 / scale) * 120 * 100) / 100}px`,
      backgroundColor: CLASS_COLORS.tokens,
    });
    expect(bars[1]).toHaveStyle({ width: `${Math.round((7667525 / scale) * 120 * 100) / 100}px` });
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
    expect(screen.getByText(/^sorted by$/)).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Sort inventory' })).toHaveValue('capacity');
  });
});

describe('Identities view (IV-12, IV-02)', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/inventory/identities');
    vi.mocked(api.getAssets).mockResolvedValue(identities);
  });
  it('lists the systems plainly: no standard chips, no repeated name, rose bars', async () => {
    render(<IdentitiesPage />);
    const table = await screen.findByRole('table', { name: 'Identity systems' });
    const name = within(table).getByRole('link', { name: '.bit' });
    expect(name).toHaveClass('text-body');
    expect(screen.queryByRole('group', { name: 'Standard filters' })).not.toBeInTheDocument();
    expect(within(table).getAllByText('.bit')).toHaveLength(1);
    expect(
      screen.getByText('4 naming systems · every identity’s record lives in its cell on CKB')
    ).toBeInTheDocument();
    // Table-rows L2-3: the 8 px AlignedBar segment carries the rose class colour.
    expect(table.querySelector('[data-segment]')).toHaveStyle({
      backgroundColor: CLASS_COLORS.identities,
    });
  });
});
