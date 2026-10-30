// F2 group `objects` (round 2): a Spore cluster against the approved Collection board — the
// Objects section is one allele gel + a Newest list with mini gels and row ↔ genotype linking
// (OB-C1 P0 page height, OB-C2, OB-C3), board section order (OB-C5), header (OB-C4), history line
// (OB-C6), holder bars (OB-C7), rail (OB-C8), coverage out of tier 1 (OB-C11) and the blueprint
// as a table (OB-C12).
import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import ClusterPage from '@/app/clusters/[clusterId]/client-page';
import { api, type GalleryObject } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
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
afterEach(() => vi.restoreAllMocks());

async function page() {
  const view = render(<ClusterPage clusterId={NERVAPE3X_CLUSTER} />);
  await screen.findByRole('heading', { level: 1, name: 'Nervape 3x' }, { timeout: 5000 });
  await screen.findByRole('table', { name: 'Newest objects' });
  return view;
}

it('makes Objects one allele gel plus the 8 newest, without per-variant button lists (OB-C1, OB-C2, OB-C3)', async () => {
  const { container } = await page();
  const objects = screen.getByRole('region', { name: 'Objects' });
  expect(objects).toHaveTextContent('90 · 6 genes · how common each variant is');
  expect(
    within(objects).getByRole('img', { name: /allele gel of 90 objects/ })
  ).toBeInTheDocument();
  expect(screen.queryByRole('region', { name: 'Collection allele frequencies' })).toBeNull();
  expect(container.querySelectorAll('[data-lane]')).toHaveLength(6);
  expect(within(objects).queryAllByRole('button', { name: /more .* variants/ })).toHaveLength(0);
  expect(api.getCollectionGallery).toHaveBeenCalledWith(NERVAPE3X_CLUSTER, {
    limit: 8,
    cursor: undefined,
    traits: {},
  });
  const table = screen.getByRole('table', { name: 'Newest objects' });
  expect(
    within(table)
      .getAllByRole('columnheader')
      .map((h) => h.textContent)
  ).toEqual(['Object', 'Id', 'Owner', 'DNA · read →', 'Minted']);
  expect(within(table).getAllByRole('row')).toHaveLength(9);
  expect(within(table).getAllByRole('img', { name: /DNA:/ })).toHaveLength(8);
  expect(objects).toHaveTextContent('hover a row to see its genotype · click a band to filter');
});

it('shows a row’s genotype on the gel while the row is hovered (OB-C2)', async () => {
  const { container } = await page();
  const overlay = container.querySelector(`[data-overlay="${NERVAPE3X_ID}"]`)!;
  expect(overlay).toHaveAttribute('opacity', '0');
  const row = screen.getAllByRole('row')[1];
  fireEvent.mouseEnter(row);
  expect(overlay).toHaveAttribute('opacity', '1');
  expect(overlay.querySelectorAll('[data-tick]')).toHaveLength(6);
  expect(row).toHaveAttribute('data-active', 'true');
  fireEvent.mouseLeave(row);
  expect(overlay).toHaveAttribute('opacity', '0');
});

it('filters the newest list by clicking a band, and clears it (OB-N2: every variant stays a filter)', async () => {
  const { container } = await page();
  fireEvent.click(container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')!);
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith(NERVAPE3X_CLUSTER, {
      limit: 8,
      cursor: undefined,
      traits: { Color: 'Calm Blue' },
    })
  );
  const chip = await screen.findByRole('button', { name: 'Clear Color = Calm Blue' });
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

it('orders sections as the board: Objects, History, Who holds it (OB-C5)', async () => {
  await page();
  const heads = screen
    .getAllByRole('heading', { level: 2 })
    .map((h) => h.textContent)
    .filter((t) => ['Objects', 'History', 'Who holds it'].includes(t ?? ''));
  expect(heads).toEqual(['Objects', 'History', 'Who holds it']);
});

it('opens with the cluster kind, its on-chain description, the composition sentence and a summary (OB-C4)', async () => {
  await page();
  const header = screen.getByRole('banner');
  expect(header).toHaveTextContent('Spore cluster · DOB/0');
  expect(header).toHaveTextContent('NVAP 3X · the cluster’s description, on chain');
  expect(header).toHaveTextContent(
    'All 90 objects are fully on-chain: records in CKB cells, pictures in Bitcoin inscriptions.'
  );
  expect(header).toHaveTextContent(
    '90 objects. 90 of their 90 holders own exactly one. Their cells hold 28,920 CKB.'
  );
});

it('gives the rail the board facts (OB-C8) and the history its minting line (OB-C6)', async () => {
  await page();
  const rail = screen.getByRole('complementary', { name: 'Facts' });
  for (const label of ['Objects', 'Holders', 'CKB held in their cells', 'Activity', 'Created'])
    expect(within(rail).getByText(label)).toBeInTheDocument();
  expect(rail).toHaveTextContent('all live · none melted');
  expect(rail).toHaveTextContent('184 · 90 mints, 94 transfers through 2026-10-24');
  expect(screen.getByRole('region', { name: 'History' })).toHaveTextContent(
    'Minted in 90 transactions · 94 transfers since'
  );
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
  const relief = within(blueprint).getByRole('row', { name: /relief/ });
  expect(relief).toHaveTextContent('1 option: NERVAPE 3X');
  expect(tier2).not.toHaveTextContent('["NERVAPE 3X"]');
});

it('draws holder bars in the objects colour (OB-C7)', async () => {
  await page();
  const holders = screen.getByRole('region', { name: 'Who holds it' });
  expect(await within(holders).findAllByTestId('count-bar')).not.toHaveLength(0);
});

it('still shows an empty Newest list honestly', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue(emptyPage());
  render(<ClusterPage clusterId={NERVAPE3X_CLUSTER} />);
  expect(await screen.findByText('No matching objects.')).toBeInTheDocument();
});
