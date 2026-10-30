// F2 group `objects` (round 2): the Object page against the approved Object board — hero summary,
// traits and composition sentence (OB-6), the sequencing gel + the collection's allele gel bridge
// (OB-4, OB-7), the three-way hover (OB-5), the cell over its life (OB-9), the collection (OB-10),
// the facts rail (OB-11), one "Live" style (OB-15) and the provenance moved to tier 2 (OB-8).
import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import { api, type ItemCellVersions } from '@/lib/api';
import { mockItemReads } from '@/__tests__/utils/item-read-fixtures';
import {
  clusterDetailProducer,
  objectCollectionDetailProducer,
  sporeCellVersionsProducer,
  sporeDetailProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import {
  NERVAPE3X_CLUSTER,
  NERVAPE3X_ID,
  nervape3xGenetics,
  nervape3xInsights,
} from '@/__tests__/fixtures/objects/nervape3x';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
vi.mock('@/src/navigation', () => ({
  useRouter: () => ({ replace: vi.fn() }),
  usePathname: () => '/objects/test',
  useSearchParams: () => new URLSearchParams(),
}));

const minted = sporeCellVersionsProducer.data[0];
function lifeVersions(): ItemCellVersions {
  const day = Date.UTC(2026, 9, 22, 6, 43, 0);
  return {
    total: 2,
    data: [
      {
        ...minted,
        isLive: false,
        consumedByTx: '0x' + 'be'.repeat(32),
        consumedAtBlock: 20801533,
        blockNumber: 20801528,
        timestamp: String(day),
        capacity: '30700000000',
        occupiedCapacity: '30700000000',
        anatomy: { capacityBytes: 8, lockBytes: 53, typeBytes: 65, dataBytes: 181 },
        lockName: 'secp256k1',
      },
      {
        ...minted,
        txHash: '0x' + 'be'.repeat(32),
        outputIndex: 1,
        blockNumber: 20801533,
        timestamp: String(day + 30_000),
        capacity: '32000000000',
        occupiedCapacity: '32000000000',
        anatomy: { capacityBytes: 8, lockBytes: 66, typeBytes: 65, dataBytes: 181 },
        lockName: 'Nervape Shadow Lock',
      },
    ],
  };
}

beforeEach(() => {
  mockItemReads();
  vi.spyOn(api, 'getSporeObject').mockResolvedValue({
    ...sporeDetailProducer,
    sporeId: NERVAPE3X_ID,
    clusterId: NERVAPE3X_CLUSTER,
    contentSize: 116,
    ownedCapacity: '32000000000',
    ownedKnowledge: '32000000000',
    mediaProfile: {
      tier: 'btc_ckb',
      sources: [
        {
          uri: 'btcfs://a94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0i0',
          scheme: 'btcfs',
          sourceLocation: 'dob_decoded_trait',
          dependencyTier: 'btc_ckb',
        },
      ],
      issues: [],
    },
  });
  vi.spyOn(api, 'getItemCellVersions').mockResolvedValue(lifeVersions());
  vi.spyOn(api, 'getObjectGenetics').mockResolvedValue(nervape3xGenetics);
  vi.spyOn(api, 'getCollectionInsights').mockResolvedValue(nervape3xInsights);
  vi.spyOn(api, 'getSporeCluster').mockResolvedValue({
    ...clusterDetailProducer,
    clusterId: NERVAPE3X_CLUSTER,
    name: 'Nervape 3x',
    sporesCount: 90,
    holdersCount: 90,
    createdAtBlock: 17222190,
    ownedCapacity: '2892000000000',
  });
  vi.spyOn(api, 'getObjectCollection').mockResolvedValue({
    ...objectCollectionDetailProducer,
    collectionId: NERVAPE3X_CLUSTER,
    name: 'Nervape 3x',
    liveCount: 90,
    totalCount: 90,
    holdersCount: 90,
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
});
afterEach(() => vi.restoreAllMocks());

async function page() {
  const view = render(<SporeDetailPage sporeId={NERVAPE3X_ID} />);
  await screen.findByRole('heading', { level: 1, name: 'Nervape 3x' }, { timeout: 5000 });
  await screen.findByLabelText('Traits');
  return view;
}

it('opens with the composition sentence, a summary of its life and its readable traits (OB-6, OB-16)', async () => {
  await page();
  const header = screen.getByRole('banner');
  expect(within(header).getByText('BTC+CKB')).toBeInTheDocument();
  expect(header).toHaveTextContent(
    'Fully on-chain: its record lives in a CKB cell, its image in a Bitcoin inscription.'
  );
  expect(
    await within(header).findByText(/^Minted .* to a secp256k1 lock, then moved/)
  ).toHaveTextContent('then moved 30 seconds later to a Nervape Shadow Lock, where it sits now.');
  expect(header).toHaveTextContent('One of 90 in Nervape 3x, each held by a different address.');
  const traits = screen.getByLabelText('Traits');
  expect([...traits.querySelectorAll('dt')].map((d) => d.textContent)).toEqual([
    'relief',
    'Color',
    'Serial',
    'Note',
  ]);
  expect(within(traits).getByText('Nature Green')).toBeInTheDocument();
  expect(within(traits).getByText('empty')).toHaveClass('text-text-dim');
});

it('draws the sequencing gel and the collection allele gel with "this one" (OB-4, OB-7)', async () => {
  const { container } = await page();
  const dna = screen.getByRole('region', { name: 'DNA' });
  expect(dna).toHaveTextContent('53 bytes · 6 genes · hover a gene');
  expect(dna).toHaveTextContent(
    'Its prev.bg gene is a pointer to a Bitcoin inscription; that is what makes it BTC+CKB.'
  );
  expect(container.querySelectorAll('[data-nibble]')).toHaveLength(106);
  expect(dna).toHaveTextContent(
    'Among Nervape 3x · its variant in each gene, and how common it is among the 90'
  );
  expect(container.querySelector('[data-overlay="me"]')!.textContent).toContain('this one');
  expect(within(dna).getByRole('link', { name: 'Open the collection →' })).toHaveAttribute(
    'href',
    expect.stringContaining(`/clusters/${NERVAPE3X_CLUSTER}`)
  );
});

it('links trait ↔ gene ↔ lane on hover, in every direction (OB-5)', async () => {
  const { container } = await page();
  const color = screen.getByLabelText('Traits').querySelector('[data-trait="Color"]')!;
  fireEvent.mouseEnter(color);
  expect(container.querySelector('[data-gene="Serial"]')).toHaveAttribute('opacity', '0.12');
  expect(container.querySelector('[data-gene="Color"]')).toHaveAttribute('opacity', '1');
  expect(container.querySelector('[data-lane="Serial"]')).toHaveAttribute('opacity', '0.2');
  expect(container.querySelector('[data-lane="Color"]')).toHaveAttribute('opacity', '1');
  fireEvent.mouseLeave(color);
  fireEvent.mouseEnter(container.querySelector('[data-lane="Serial"]')!);
  expect(screen.getByLabelText('Traits').querySelector('[data-trait="Serial"]')).toHaveAttribute(
    'data-active',
    'true'
  );
  expect(container.querySelector('[data-gene="Color"]')).toHaveAttribute('opacity', '0.12');
  fireEvent.mouseLeave(container.querySelector('[data-lane="Serial"]')!);
  fireEvent.mouseEnter(container.querySelector('[data-gene="Note"]')!);
  expect(screen.getByLabelText('Traits').querySelector('[data-trait="Note"]')).toHaveAttribute(
    'data-active',
    'true'
  );
  expect(container.querySelector('[data-lane="Color"]')).toHaveAttribute('opacity', '0.2');
});

it('shows its cell at minting and now, to scale, with what changed (OB-9)', async () => {
  await page();
  const cell = await screen.findByRole('region', { name: 'CKB held in its cell' });
  expect(cell).toHaveTextContent('1 CKB per byte · all occupied, none free');
  expect(cell).toHaveTextContent(
    'The object’s cell holds 320 CKB. It held 307 at minting; the lock grew by 13 bytes, so the cell grew by 13 CKB when it moved.'
  );
  const rows = within(cell).getAllByRole('article');
  expect(rows.map((r) => r.querySelector('a')!.textContent)).toEqual(['At minting', 'Now']);
  expect(rows[0].querySelector('[data-scaled]')).toHaveStyle({ width: '95.938%' });
  expect(rows[1].querySelector('[data-scaled]')).toHaveStyle({ width: '100%' });
});

it('places it in its collection with the composition count and cluster cell (OB-10)', async () => {
  await page();
  const coll = await screen.findByRole('region', { name: 'In Nervape 3x' });
  expect(coll).toHaveTextContent('90 objects, all live');
  expect(coll).toHaveTextContent('90 holders');
  expect(coll).toHaveTextContent('90 of 90');
  expect(within(coll).getByText('BTC+CKB')).toBeInTheDocument();
  expect(coll).toHaveTextContent('28,920 CKB held in their cells');
  expect(coll).toHaveTextContent('created at #17,222,190');
});

it('lists the board facts and one Live style (OB-11, OB-15)', async () => {
  await page();
  const rail = screen.getByRole('complementary', { name: 'Facts' });
  for (const label of [
    'Owner',
    'Collection',
    'Minted',
    'Last moved',
    'CKB held in its cell',
    'Content',
  ])
    expect(within(rail).getByText(label)).toBeInTheDocument();
  expect(rail).toHaveTextContent('Nervape Shadow Lock');
  expect(await within(rail).findByText('1 transfer since minting')).toBeInTheDocument();
  expect(rail).toHaveTextContent('2026-10-22 14:43:00 UTC+8');
  expect(within(rail).getByText('Nervape 3x')).toBeInTheDocument();
  expect(screen.getByText('Live')).toHaveClass('text-jade', 'border-jade/40');
});

it('keeps decoder provenance out of tier 1 and in the DNA evidence below (OB-8)', async () => {
  const { container } = await page();
  const tier1 = container.querySelector('[data-tier="1"]')!;
  expect(tier1).not.toHaveTextContent('original decoder binary hash was not recorded');
  expect(tier1).not.toHaveTextContent('Cached VM result');
  const tier2 = container.querySelector('[data-tier="2"]')!;
  expect(tier2).toHaveTextContent('original decoder binary hash was not recorded');
  expect(tier2).toHaveTextContent('Declared bytes 0 + 32');
});
