// F2 group `objects` (round 2): the Object page against the approved Object board — hero summary,
// traits and composition sentence (OB-6), the sequencing gel + the collection's allele gel bridge
// (OB-4, OB-7), the three-way hover (OB-5), the cell over its life (OB-9), the collection (OB-10),
// the facts rail (OB-11), one "Live" style (OB-15) and the provenance moved to tier 2 (OB-8).
import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import { api, type ItemCellVersions } from '@/lib/api';
import { mockItemReads } from '@/__tests__/utils/item-read-fixtures';
import { ledeRestatements } from '@/__tests__/utils/lede-rule';
import { expectPrompt, promptNote } from '@/__tests__/utils/prompt-heading';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { expectTier1Budget } from '@/__tests__/utils/hierarchy';
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
  // the gels lay themselves out for their measured box (V2)
  stubChartWidth(896);
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
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function page() {
  const view = render(<SporeDetailPage sporeId={NERVAPE3X_ID} />);
  await screen.findByRole('heading', { level: 1, name: 'Nervape 3x' });
  await screen.findByLabelText('Traits');
  return view;
}

it('opens with one hero section — name, status, id and the composition sentence as its lede — then its readable traits; its life story is in tier 2 (OB-6, OB-16, F4, R13)', async () => {
  const { container } = await page();
  const header = screen.getByRole('banner');
  const hero = within(header).getByRole('region', { name: 'Nervape 3x' });
  expect(within(hero).getByText('BTC+CKB')).toBeInTheDocument();
  expect(within(hero).getByText('Live')).toBeInTheDocument();
  const lede = within(hero).getByText(
    'Fully on-chain: its record lives in a CKB cell, its image in a Bitcoin inscription.'
  );
  expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
  expect(lede.textContent!.length).toBeLessThanOrEqual(90);
  const traits = screen.getByLabelText('Traits');
  expect(hero.contains(traits)).toBe(false);
  expect([...traits.querySelectorAll('dt')].map((d) => d.textContent)).toEqual([
    'relief',
    'Color',
    'Serial',
    'Note',
  ]);
  expect(within(traits).getByText('Nature Green')).toBeInTheDocument();
  expect(within(traits).getByText('empty')).toHaveClass('text-text-dim');
  // the life summary and the collection sentence: tier 2, every word kept
  const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
  const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
  const story = await within(tier2).findByText(
    // Lane P: the path only; the dates are the metrics, the ages the sentence's hover.
    /^Minted to a secp256k1 lock, then moved/
  );
  expect(story).toHaveTextContent(
    'then moved 30 seconds later to a Nervape Shadow Lock, where it sits now.'
  );
  expect(story).toHaveAttribute(
    'title',
    expect.stringMatching(/^minted .+ ago · last moved .+ ago$/)
  );
  expect(tier2).toHaveTextContent('One of 90 in Nervape 3x, each held by a different address.');
  expect(tier1).not.toHaveTextContent(/^Minted \d/);
  expect(tier1).not.toHaveTextContent('each held by a different address');
});

it('keeps tier 1 within the prose budget: one lede and captions under the evidence (F4, §0)', async () => {
  const { container } = await page();
  await screen.findByRole('region', { name: 'CKB held in its cell' });
  await screen.findByRole('region', { name: 'In Nervape 3x' });
  expectTier1Budget(container, { maxProseBlocks: 3, maxChars: 160 });
});

it('draws the sequencing gel and the collection allele gel with "this one" (OB-4, OB-7)', async () => {
  const { container } = await page();
  // Phase 5 (A13): the size and the genes are the DNA command's args, and name its section.
  const dna = screen.getByRole('region', { name: 'DNA 53 bytes · 6 genes' });
  // the hint is the readout line's "tap or hover" under the map (R6), not a header suffix
  expect(dna).not.toHaveTextContent('6 genes · hover a gene');
  expect(within(dna).getByRole('status', { name: 'Gene readout' })).toHaveTextContent(
    'tap or hover'
  );
  // evidence first: one caption under the plate, the full paragraph in tier 2 (F4, R13)
  const caption = dna.querySelector('p[data-role="caption"]')!;
  expect(caption).toHaveTextContent(
    'Its traits are read from 53 bytes of DNA; its prev.bg gene points to a Bitcoin inscription, which makes it BTC+CKB.'
  );
  expect(caption).toHaveClass('text-body', 'max-w-[640px]');
  expect(caption.textContent!.length).toBeLessThanOrEqual(160);
  const plate = container.querySelector('[data-nibble]')!;
  expect(plate.compareDocumentPosition(caption) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(dna).not.toHaveTextContent('The object’s traits are not stored as text.');
  expect(container.querySelector('[data-tier="2"]')).toHaveTextContent(
    'The object’s traits are not stored as text. It carries 53 bytes of DNA, and the collection’s decoder reads each gene to express a trait. Its prev.bg gene is a pointer to a Bitcoin inscription; that is what makes it BTC+CKB.'
  );
  expect(container.querySelectorAll('[data-nibble]')).toHaveLength(106);
  // C10: the collection's gel is its own section beside the DNA, so each has one hint.
  const among = screen.getByRole('region', { name: 'Among Nervape 3x' });
  expect(dna.contains(among)).toBe(false);
  expect(among).toHaveTextContent(
    'Among Nervape 3x · its variant in each gene, and how common it is among the 90'
  );
  expect(container.querySelector('[data-overlay="me"]')!.textContent).toContain('this one');
  expect(within(among).getByRole('link', { name: 'Open the collection →' })).toHaveAttribute(
    'href',
    expect.stringContaining(`/clusters/${NERVAPE3X_CLUSTER}`)
  );
  // C9: only the genes that vary among the 90 get a lane; the rest are named once.
  expect(
    [...among.querySelectorAll('[data-lane]')].map((l) => l.getAttribute('data-lane'))
  ).toEqual(['prev.bg', 'Color', 'Serial']);
  expect(among.querySelector('[data-role="caption"]')).toHaveTextContent(
    'Type is image, relief is NERVAPE 3X and Note is empty for every object, so none of them gets a lane.'
  );
  for (const section of [dna, among]) {
    const hints = [...section.querySelectorAll('*')].filter((el) =>
      [...el.childNodes].some(
        (n) => n.nodeType === Node.TEXT_NODE && /\b(tap|hover)\b/i.test(n.textContent ?? '')
      )
    );
    expect(hints).toHaveLength(1);
  }
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
  // R11: no coloured words in tier 1; the bars keep the occupied colour
  expect(within(cell).getByText('all occupied')).not.toHaveClass('text-jade');
  // the cell sentence is the caption under the evidence (F4); the lock note is tier 2
  const caption = cell.querySelector('p[data-role="caption"]')!;
  expect(caption).toHaveTextContent(
    'The object’s cell holds 320 CKB. It held 307 at minting; the lock grew by 13 bytes, so the cell grew by 13 CKB when it moved.'
  );
  expect(caption).toHaveClass('text-body', 'max-w-[640px]');
  expect(caption).toBe(cell.lastElementChild);
  expect(cell.textContent).not.toMatch(/Lock: secp256k1/);
  expect(document.querySelector('[data-tier="2"]')).toHaveTextContent(
    'Lock: secp256k1 throughout. Type and data never changed.'
  );
  const rows = within(cell).getAllByRole('article');
  expect(rows.map((r) => r.querySelector('a')!.textContent)).toEqual(['At minting', 'Now']);
  expect(rows[0].querySelector('[data-scaled]')).toHaveStyle({ width: '95.938%' });
  expect(rows[1].querySelector('[data-scaled]')).toHaveStyle({ width: '100%' });
});

it('places it in its collection with the composition count and cluster cell (OB-10)', async () => {
  await page();
  const coll = await screen.findByRole('region', { name: 'In Nervape 3x' });
  // Phase 5 (A11): the collection, run on its name; the link to it is the note.
  const heading = within(coll).getByRole('heading', { name: 'Collection Nervape 3x' });
  expectPrompt(heading, 'Collection', 'Nervape 3x');
  expect(promptNote(heading)).toContainElement(
    within(coll).getByRole('link', { name: 'Open the collection →' })
  );
  expect(coll).toHaveTextContent('90 objects, all live');
  expect(coll).toHaveTextContent('90 holders');
  expect(coll).toHaveTextContent('90 of 90');
  expect(within(coll).getByText('BTC+CKB')).toBeInTheDocument();
  expect(coll).toHaveTextContent('28,920 CKB held in their cells');
  expect(coll).toHaveTextContent('created at #17,222,190');
  // Lane L: the block the cluster was created in is a link.
  expect(within(coll).getByRole('link', { name: '#17,222,190' })).toHaveAttribute(
    'href',
    '/mainnet/blocks/17222190'
  );
});

it('lists the board facts as the metrics row and one Live style (OB-11, OB-15, C7)', async () => {
  const { container } = await page();
  expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
  const rail = screen.getByRole('region', { name: 'Metrics' });
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
  // C7: the metric shows the date; the time of day stays in the metric's title.
  expect(rail).toHaveTextContent('2026-10-22');
  expect(within(rail).getByText('Minted').parentElement).toHaveAttribute(
    'title',
    expect.stringContaining('2026-10-22 14:43:00 UTC+8')
  );
  expect(within(rail).getByText('Nervape 3x')).toBeInTheDocument();
  expect(ledeRestatements(container)).toEqual([]);
  expect(screen.getByText('Live')).toHaveClass('text-jade', 'border-jade/40');
});

it('keeps decoder provenance out of tier 1 and in the DNA evidence below (OB-8)', async () => {
  const { container } = await page();
  const tier1 = container.querySelector('[data-tier="1"]')!;
  expect(tier1).not.toHaveTextContent('original decoder binary hash was not recorded');
  expect(tier1).not.toHaveTextContent('Cached VM result');
  const tier2 = container.querySelector('[data-tier="2"]')!;
  expect(tier2).toHaveTextContent('original decoder binary hash was not recorded');
  // Lane E (C5): one row per gene; the declared range in words is the row's title.
  const genes = within(tier2 as HTMLElement).getByRole('table', { name: 'DNA genes' });
  expect(within(genes).getByRole('row', { name: /^prev\.bg/ })).toHaveAttribute(
    'title',
    expect.stringContaining('Declared bytes 0 + 32')
  );
});

// OB-14 (Object.dc.html): the object's media sits beside the title in the hero (300 px column),
// and tier 2 reads as two columns side by side.
it('puts the media beside the title and lays tier 2 out in two columns (OB-14)', async () => {
  const { container } = await page();
  const hero = container.querySelector('[data-hero-media]')!;
  expect(hero).not.toBeNull();
  expect(hero).toContainElement(screen.getByRole('region', { name: 'Object media' }));
  expect(hero.closest('header')).toContainElement(
    screen.getByRole('heading', { level: 1, name: 'Nervape 3x' })
  );
  expect(hero.parentElement).toHaveClass('md:grid-cols-[300px_minmax(0,1fr)]');
  const columns = container.querySelector('[data-tier="2"] [data-tier2-columns]')!;
  expect(columns).toHaveClass('md:grid-cols-2');
  expect(columns.children).toHaveLength(2);
  expect(columns.children[0]).toHaveTextContent('original decoder binary hash was not recorded');
  // the sentences that left tier 1 head the shorter, DNA column (page height)
  expect(
    within(columns.children[0] as HTMLElement).getByRole('heading', {
      name: 'This object, in words',
    })
  ).toBeInTheDocument();
  expect(columns.children[1]).toHaveTextContent(/[Cc]ell data/);
});

// Responsive review B#23: on a phone the first 500 px was the Bitcoin provenance card; the name,
// Live badge, composition and summary sentence come first and the media after them.
it('reads the name, status, composition and summary before the media, in DOM order', async () => {
  const { container } = await page();
  const before = (a: Element, b: Element) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
  const media = container.querySelector('[data-hero-media]')!;
  const header = screen.getByRole('banner');
  for (const first of [
    screen.getByRole('heading', { level: 1, name: 'Nervape 3x' }),
    within(header).getByText('Live'),
    within(header).getByText('BTC+CKB'),
    within(header).getByText(/^Fully on-chain: /),
  ])
    expect(before(first, media), first.textContent ?? '').toBe(true);
  // From md up the media still takes the left column beside the title.
  expect(media).toHaveClass('md:col-start-1', 'md:row-start-1');
});
