// F2 group `objects` (round 2): a Spore cluster against the approved Collection board — the
// Objects section is one allele gel + a Newest list with mini gels and row ↔ genotype linking
// (OB-C1 P0 page height, OB-C2, OB-C3), board section order (OB-C5), header (OB-C4), history line
// (OB-C6), holder bars (OB-C7), rail (OB-C8), coverage out of tier 1 (OB-C11) and the blueprint
// as a table (OB-C12).
import { CLASS_COLORS } from '@/lib/design-tokens';
import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import ClusterPage from '@/app/clusters/[clusterId]/client-page';
import { api, type GalleryObject } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { ledeRestatements } from '@/__tests__/utils/lede-rule';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import { collectionActivityFeedProducer } from '@/__tests__/msw/fixtures/collection-feed';
import {
  clusterDetailProducer,
  collectionGalleryFilteredProducer,
  objectCollectionDetailProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import {
  NERVAPE3X_CLUSTER,
  NERVAPE3X_ID,
  nervape3xGenetics,
  nervape3xInsights,
  nervape3xOtherTraits,
} from '@/__tests__/fixtures/objects/nervape3x';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));

const template = collectionGalleryFilteredProducer.data[0];
const rows: GalleryObject[] = [
  [NERVAPE3X_ID, nervape3xGenetics.cachedVmTraits.traits] as const,
  ...Object.entries(nervape3xOtherTraits),
].map(([sporeId, traits], i) => ({
  ...template,
  sporeId,
  clusterId: NERVAPE3X_CLUSTER,
  number: String(90 - i),
  dnaHex: nervape3xGenetics.dnaHex,
  traits: [...traits],
  createdAtBlock: 20801528 - i,
  mediaProfile: { tier: 'btc_ckb', sources: [], issues: [] },
}));

beforeEach(() => {
  mockItemReads();
  // the gels lay themselves out for their measured box (V2)
  stubChartWidth(1248);
  vi.spyOn(api, 'getSporeCluster').mockResolvedValue({
    ...clusterDetailProducer,
    clusterId: NERVAPE3X_CLUSTER,
    name: 'Nervape 3x',
    description: '{"description":"NVAP 3X"}',
    sporesCount: 90,
    holdersCount: 90,
    activitiesCount: 184,
    createdAtBlock: 17222190,
  });
  vi.spyOn(api, 'getObjectCollection').mockResolvedValue({
    ...objectCollectionDetailProducer,
    collectionId: NERVAPE3X_CLUSTER,
    liveCount: 90,
    totalCount: 90,
    holdersCount: 90,
    activitiesCount: 184,
    ownedCapacity: '2892000000000',
    ownedKnowledge: '2892000000000',
    composition: {
      tier: 'btc_ckb',
      onchainCount: 90,
      pureCkbCount: 0,
      decentralizedMixtureCount: 0,
      centralizedMixtureCount: 0,
      unknownCount: 0,
      onchainRatio: '1.0000',
    },
  });
  vi.spyOn(api, 'getCollectionInsights').mockResolvedValue(nervape3xInsights);
  vi.spyOn(api, 'getSporeClusterCapacityChart').mockResolvedValue({
    source: 'indexedChainTimeUtc8',
    completeThrough: '2026-10-29',
    anchor: {
      blockNumber: 20876500,
      blockHash: '0x' + 'aa'.repeat(32),
      chainTime: '1793300000000',
    },
    title: 'Collection capacity',
    data: [
      { date: '2026-10-21', values: { used: '0', unused: '0' } },
      { date: '2026-10-22', values: { used: '2892000000000', unused: '0' } },
    ],
    series: [
      { key: 'used', label: 'Used', color: '#f59e0b' },
      { key: 'unused', label: 'Unused', color: '#00c389' },
    ],
  });
  vi.spyOn(api, 'getCollectionGallery').mockResolvedValue({
    data: rows,
    total: 90,
    limit: 8,
    hasMore: true,
    nextCursor: 'older',
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function page() {
  const view = render(<ClusterPage clusterId={NERVAPE3X_CLUSTER} />);
  await screen.findByRole('heading', { level: 1, name: 'Nervape 3x' }, { timeout: 5000 });
  await screen.findByRole('table', { name: 'Newest objects' }, { timeout: 5000 });
  return view;
}

// Consistency round C9 / C11 (Main.dc.html): lanes only for the genes that vary, named in words
// where a DOB/1 key has one; the rest named in one caption; the gel beside the twenty newest.
it('makes Objects the gel of the genes that vary beside the 20 newest (OB-C1, C9, C11)', async () => {
  const { container } = await page();
  const objects = screen.getByRole('region', { name: 'Objects' });
  expect(within(objects).getByRole('heading', { level: 2 })).toHaveTextContent(
    'Objects · 90 · three genes vary'
  );
  expect(
    within(objects).getByRole('img', { name: /allele gel of 90 objects/ })
  ).toBeInTheDocument();
  expect(screen.queryByRole('region', { name: 'Collection allele frequencies' })).toBeNull();
  expect(
    [...container.querySelectorAll('[data-lane]')].map((l) => l.getAttribute('data-lane'))
  ).toEqual(['prev.bg', 'Color', 'Serial']);
  // prev.bg reads as a word on its lane.
  expect(container.querySelector('[data-lane="prev.bg"] [data-gene-caption]')).toHaveTextContent(
    'Background'
  );
  expect(objects.querySelector('[data-role="caption"]')).toHaveTextContent(
    'Type is image, relief is NERVAPE 3X and Note is empty for every object, so none of them gets a lane.'
  );
  expect(within(objects).queryAllByRole('button', { name: /more .* variants/ })).toHaveLength(0);
  expect(api.getCollectionGallery).toHaveBeenCalledWith(NERVAPE3X_CLUSTER, {
    limit: 20,
    cursor: undefined,
    traits: {},
  });
  const table = screen.getByRole('table', { name: 'Newest objects' });
  expect(
    within(table)
      .getAllByRole('columnheader')
      .map((h) => h.textContent)
  ).toEqual(['Object', 'Background', 'Color', 'Made of', 'Owner', 'Minted']);
  expect(within(table).getAllByRole('row')).toHaveLength(9);
  expect(within(table).getAllByText('BTC+CKB')).toHaveLength(8);
  // C10: one hint in the section — the readout row's.
  const hints = [...objects.querySelectorAll('*')].filter((el) =>
    [...el.childNodes].some(
      (n) => n.nodeType === Node.TEXT_NODE && /\b(tap|hover)\b/i.test(n.textContent ?? '')
    )
  );
  expect(hints).toHaveLength(1);
});

it('shows a row’s genotype on the gel while the row is hovered (OB-C2)', async () => {
  const { container } = await page();
  const overlay = container.querySelector(`[data-overlay="${NERVAPE3X_ID}"]`)!;
  expect(overlay).toHaveAttribute('opacity', '0');
  const row = screen.getAllByRole('row')[1];
  fireEvent.mouseEnter(row);
  expect(overlay).toHaveAttribute('opacity', '1');
  expect(overlay.querySelectorAll('[data-tick]')).toHaveLength(3);
  expect(row).toHaveAttribute('data-active', 'true');
  fireEvent.mouseLeave(row);
  expect(overlay).toHaveAttribute('opacity', '0');
});

it('filters the newest list by clicking a band, and clears it (OB-N2: every variant stays a filter)', async () => {
  const { container } = await page();
  fireEvent.click(container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')!);
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith(NERVAPE3X_CLUSTER, {
      limit: 20,
      cursor: undefined,
      traits: { Color: 'Calm Blue' },
    })
  );
  const chip = await screen.findByRole(
    'button',
    { name: 'Clear Color = Calm Blue' },
    { timeout: 5000 }
  );
  expect(container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')).toHaveAttribute(
    'stroke',
    '#ffffff'
  );
  fireEvent.click(chip);
  // back to the unfiltered list (served from the query cache)
  await waitFor(() =>
    expect(screen.queryByRole('button', { name: /^Clear / })).not.toBeInTheDocument()
  );
  expect(
    container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')
  ).not.toHaveAttribute('stroke');
});

// Main.dc.html: the history is the hero's picture; then Objects; then who holds it beside the
// recent activity.
it('orders sections as the board: History in the hero, Objects, Who holds it beside Recent activity', async () => {
  await page();
  const heads = screen
    .getAllByRole('heading', { level: 2 })
    .map((h) => (h.textContent ?? '').split(' · ')[0])
    .filter((t) => ['Objects', 'History', 'Who holds it', 'Recent activity'].includes(t));
  expect(heads).toEqual(['History', 'Objects', 'Who holds it', 'Recent activity']);
  const holders = screen.getByRole('region', { name: 'Who holds it' });
  expect(holders.parentElement).toHaveClass('lg:grid-cols-2');
  expect(holders.parentElement).toContainElement(
    screen.getByRole('region', { name: 'Recent activity' })
  );
  // Screen 3 is the shell's feed slot: lists of rows follow the tier-1 body, before tier 2.
  const tier1 = document.querySelector('[data-tier="1"]')!;
  expect(tier1.contains(holders)).toBe(false);
  expect(tier1.compareDocumentPosition(holders) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it('opens Recent activity with five rows and two links that open the full feed in place', async () => {
  vi.spyOn(api, 'getCollectionActivityFeed').mockImplementation(async (_kind, _id, p) => ({
    ...collectionActivityFeedProducer,
    data: Array.from({ length: p?.limit ?? 50 }, (_, i) => ({
      ...collectionActivityFeedProducer.data[0],
      txHash: `0x${(i + 1).toString(16).padStart(64, '0')}`,
    })),
  }));
  await page();
  const recent = screen.getByRole('region', { name: 'Recent activity' });
  await waitFor(() => expect(within(recent).getAllByRole('article')).toHaveLength(5));
  expect(api.getCollectionActivityFeed).toHaveBeenCalledWith('spore', NERVAPE3X_CLUSTER, {
    limit: 5,
  });
  expect(within(recent).queryByRole('tab')).toBeNull();
  fireEvent.click(within(recent).getByRole('button', { name: '184 transactions →' }));
  expect(await screen.findByRole('tab', { name: 'Transactions' })).toHaveAttribute(
    'aria-selected',
    'true'
  );
  expect(screen.queryByRole('region', { name: 'Recent activity' })).toBeNull();
});

// Consistency round (Main.dc.html): the lede is the kind and what every object is made of; the
// description is a quote; the hero's figure, the composition block and the metrics carry the rest.
it('opens with the cluster kind, its on-chain description, its composition and a summary (OB-C4, C7, C8)', async () => {
  const { container } = await page();
  // Consistency plan C1: the tab names the cluster.
  expect(document.title).toBe('Nervape 3x · cluster · CKBadger');
  const header = screen.getByRole('banner');
  const lede = header.querySelector('[data-role="lede"]')!;
  expect(lede).toHaveTextContent(
    'Spore cluster · DOB/0 · every object is BTC+CKB: its record in a CKB cell, its picture in a Bitcoin inscription.'
  );
  // Responsive fix plan 2026-11-01 (A12e, report B#21): the kind is one unit, so "Spore cluster ·"
  // never dangles at a line end.
  expect(lede.querySelector('[data-role="collection-kind"]')).toHaveClass('whitespace-nowrap');
  expect(within(header).getByText('NVAP 3X')).toHaveAttribute(
    'title',
    'the cluster’s description, on chain'
  );
  const hero = header.querySelector('[data-role="hero"]')!;
  expect(hero.querySelector('[data-role="figure"]')).toHaveTextContent(
    'objects live90 all live · none burned'
  );
  expect(hero).toHaveTextContent('28,920.00000000 CKB held in their cells');
  expect(
    within(hero as HTMLElement).getByRole('group', { name: 'Object composition' })
  ).toHaveTextContent(
    '90 of 90 fully on-chain · permanent and verifiable, as long as CKB and Bitcoin exist'
  );
  expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent(
    'Holders90 · 90 hold one'
  );
  expect(ledeRestatements(container)).toEqual([]);
});

it('gives the metrics row the board facts (OB-C8, C7) and the history its minting line (OB-C6)', async () => {
  await page();
  expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
  const rail = screen.getByRole('region', { name: 'Metrics' });
  expect(
    within(rail)
      .getAllByRole('term')
      .map((t) => t.textContent)
  ).toEqual(['Holders', 'Transactions', 'Created', 'Cluster cell held by']);
  expect(rail).toHaveTextContent('184 · 90 mints · 94 transfers · 0 burns');
  expect(within(rail).getByText('Transactions').parentElement).toHaveAttribute(
    'title',
    'mints, transfers and burns counted by complete week through 2026-10-24'
  );
  const history = screen.getByRole('region', { name: 'History' });
  expect(history).toHaveTextContent('Minted in 90 transactions · 94 transfers since');
  // Consistency plan C12: the H-multiplier is never bare; its definition reads with it.
  expect(history.querySelector('[data-role="h-multiplier-reading"]')).toHaveTextContent(
    'H-multiplier ×1.00: CKB held ÷ occupied'
  );
});

it('says "burned" for the objects no longer live, Spore\'s "melted" kept in the title (C12)', async () => {
  const before = await api.getObjectCollection(NERVAPE3X_CLUSTER);
  vi.mocked(api.getObjectCollection).mockResolvedValue({
    ...before,
    liveCount: 88,
    totalCount: 90,
    // C8: the composition counts the live objects, so it counts 88 too (the set must add up).
    composition: { ...before.composition!, onchainCount: 88 },
  });
  const { container } = await page();
  const figure = container.querySelector('[data-role="figure"]') as HTMLElement;
  const line = within(figure).getByText('of 90 minted · 2 burned');
  expect(line).toHaveAttribute('title', 'melted, in Spore’s words');
  expect(container.textContent).not.toMatch(/melted/);
});

it('keeps decode coverage and the decoder blueprint in tier 2, as a table (OB-C11, OB-C12)', async () => {
  const { container } = await page();
  const tier1 = container.querySelector('[data-tier="1"]')!;
  expect(tier1).not.toHaveTextContent(/decoded \/ .* DOB items/);
  const tier2 = container.querySelector('[data-tier="2"]')!;
  expect(tier2).toHaveTextContent('90 of 90 DOB objects decoded · 0 pending · 0 failed');
  const blueprint = await within(tier2 as HTMLElement).findByRole('table', {
    name: 'DOB blueprint',
  });
  const prevBg = within(blueprint).getByRole('row', { name: /prev\.bg/ });
  expect(prevBg).toHaveTextContent('bytes 0–31');
  expect(prevBg).toHaveTextContent('btcfs');
  // Lane E (C5): the rule cell names its options; the last cell says what the 90 objects show.
  const relief = within(blueprint).getByRole('row', { name: /relief/ });
  expect(relief).toHaveTextContent('options · NERVAPE 3X');
  expect(relief).toHaveTextContent('NERVAPE 3X, for all');
  expect(tier2).not.toHaveTextContent('["NERVAPE 3X"]');
  // one real object, read through the pattern (the newest gallery object)
  expect(
    await within(tier2 as HTMLElement).findByRole('group', { name: 'One object, read this way' })
  ).toHaveTextContent('relief');
});

// Responsive plan 2026-11-01 (the leftover after lane V): the blueprint table had a 480 px floor
// and scrolled sideways in a phone column. It is a ListTable now, so a column too narrow for its
// columns gets one phone row per gene with every column on its lines (RESPONSIVE_LAYOUT rules 1, 2).
it('lays the decoder blueprint out as phone rows in a 358 px column, nothing scrolling sideways', async () => {
  atContainerWidth(358);
  const { container } = await page();
  const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
  const list = await within(tier2).findByRole('list', { name: 'DOB blueprint, compact' });
  expect(within(tier2).queryByRole('table', { name: 'DOB blueprint' })).toBeNull();
  // Lane E (C5): the section is "How an object is read" (was "Current decoder blueprint").
  const blueprint = within(tier2).getByRole('region', { name: 'How an object is read' });
  expect(blueprint.querySelector('.overflow-x-auto, .min-w-\\[480px\\]')).toBeNull();
  const rows = within(list).getAllByRole('listitem');
  const prevBg = rows.find((r) => r.textContent?.includes('prev.bg')) as HTMLElement;
  expect(prevBg).toHaveTextContent('bytes 0–31');
  expect(prevBg).toHaveTextContent('btcfs');
  // the phone row keeps every column: gene and bytes, the rule, what the objects show — the last
  // on its own wrapping line (as a right part, a long reading pushed a 390 px page 4 px wide)
  expect(
    [...prevBg.querySelectorAll('[data-line]')].map((l) => l.getAttribute('data-line'))
  ).toEqual(['gene', 'rule', 'found']);
  expect(prevBg.querySelector('[data-line="found"] [data-part="right"]')).toBeNull();
  const relief = rows.find((r) => r.textContent?.includes('relief')) as HTMLElement;
  expect(relief).toHaveTextContent('options · NERVAPE 3X');
  expect(relief).toHaveTextContent('NERVAPE 3X, for all');
});

it('draws holder bars in the objects colour (OB-C7)', async () => {
  await page();
  const holders = screen.getByRole('region', { name: 'Who holds it' });
  // Table-rows L2-4: the counts are 8 px AlignedBars in the objects colour.
  const table = await within(holders).findByRole('table', { name: 'Top 10 collection holders' });
  const bars = table.querySelectorAll('[data-segment]');
  expect(bars).not.toHaveLength(0);
  expect(bars[0]).toHaveStyle({ backgroundColor: CLASS_COLORS.objects });
});

it('still shows an empty Newest list honestly', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue(emptyPage());
  render(<ClusterPage clusterId={NERVAPE3X_CLUSTER} />);
  expect(
    await screen.findByText('No matching objects.', undefined, { timeout: 5000 })
  ).toBeInTheDocument();
});

// Visual pass on the live services (2026-10-30).
it('marks every collection activity row as an object row, as the board does', async () => {
  // An RGB++ row classifies as a protocol action (no class mark of its own); on a collection
  // page it is still a row about this collection's objects (live: Nervape's RGB++ transfers).
  const row = collectionActivityFeedProducer.data[0];
  vi.spyOn(api, 'getCollectionActivityFeed').mockResolvedValue({
    ...collectionActivityFeedProducer,
    data: [
      {
        ...row,
        protocolActions: [
          { protocol: 'rgbpp', protocolName: 'RGB++', action: 'transfer', metadata: {} },
        ],
      },
    ],
  });
  await page();
  await screen.findAllByRole('article', undefined, { timeout: 5000 });
  const rows = [...document.querySelectorAll('[data-feed-row]')];
  expect(rows.length).toBeGreaterThan(0);
  for (const row of rows)
    expect(row.querySelector('[data-role="headline"] [data-class="objects"]')).not.toBeNull();
});

it('explains occupied and free once in History, and keeps Newest rows compact at 390', async () => {
  await page();
  const history = screen.getByRole('region', { name: 'History' });
  expect(
    history.textContent!.match(/beyond that byte cost|parked in the same cells/g)
  ).toHaveLength(1);
  const row = screen.getAllByRole('row')[1];
  expect(row).toHaveClass('grid-cols-[minmax(0,1fr)_auto]');
});
