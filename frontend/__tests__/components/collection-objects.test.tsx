// F2 group `objects` (round 2): the Collection board's Objects section (allele gel + Newest).
// The two tests of the retired 24-card gallery are kept here against the new list.
import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { CollectionObjects, DecodeCoverage } from '@/components/asset/collection-objects';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  collectionGalleryFilteredProducer,
  collectionInsightsPendingProducer,
  collectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
beforeEach(() => {
  mockItemReads();
  // the allele gel lays itself out for its measured box (V2); 1248 px is the 1280 column
  stubChartWidth(1248);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it('keeps multiple values of one trait on one row without React identity collisions', async () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([
      {
        ...collectionGalleryFilteredProducer.data[0],
        traits: [
          { name: 'Face', value: 'Blue' },
          { name: 'Face', value: 'Red' },
        ],
      },
    ])
  );
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  const table = await screen.findByRole('table', { name: 'Newest objects' });
  expect(within(table).getAllByRole('row')).toHaveLength(2);
  expect(log.mock.calls.flat().join(' ')).not.toMatch(/same key|unique.*key/);
});

it('passes the whole-collection trait predicate before cursor pagination', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue({
    ...collectionGalleryFilteredProducer,
    hasMore: true,
    nextCursor: 'older',
  });
  const { container } = render(
    <CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />
  );
  await screen.findByRole('table', { name: 'Newest objects' });
  fireEvent.click(container.querySelector('[data-allele="Blue"]')!);
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith('collection', {
      limit: 8,
      cursor: undefined,
      traits: { Face: 'Blue' },
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith('collection', {
      limit: 8,
      cursor: 'older',
      traits: { Face: 'Blue' },
    })
  );
});

it('offers no gel or filters while decoding is incomplete, and says how far it is', async () => {
  const { container } = render(
    <CollectionObjects id="collection" title="Genes" insights={collectionInsightsPendingProducer} />
  );
  await screen.findByRole('table', { name: 'Newest objects' });
  expect(container.querySelector('[data-lane]')).toBeNull();
  expect(screen.getByRole('status')).toHaveTextContent(
    'Allele frequencies and filtering await complete decoding'
  );
});

it('writes decode coverage with thousands separators (OB-C11)', () => {
  render(
    <DecodeCoverage
      genetics={{
        ...collectionInsightsProducer.genetics,
        liveItems: 2777,
        eligibleItems: 2777,
        decodedItems: 2777,
      }}
    />
  );
  expect(
    screen.getByText(/2,777 of 2,777 DOB objects decoded · 0 pending · 0 failed/)
  ).toBeInTheDocument();
});

// Round 3 (contracts R-A · R-2, OB-C3): the owner's lock kind and the mint date on Newest rows.
it('names the owner lock kind and dates the mint in UTC+8 on each Newest row', async () => {
  const base = collectionGalleryFilteredProducer.data[0];
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([
      // 1700000000100 ms = 2023-11-14 22:13:20 UTC = 2023-11-15 06:13:20 UTC+8.
      { ...base, chainTime: '1700000000100', lockKind: 'SECP256K1/blake160' },
      {
        ...base,
        sporeId: `0x${'03'.repeat(32)}`,
        chainTime: '1700000000100',
        lockCodeHash: `0x${'43'.repeat(32)}`,
        lockKind: null,
      },
    ])
  );
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  const table = await screen.findByRole('table', { name: 'Newest objects' });
  const [, named, unnamed] = within(table).getAllByRole('row');
  expect(named).toHaveTextContent('SECP256K1/blake160');
  // An unnamed lock shows its code hash, never a guessed kind.
  expect(unnamed).toHaveTextContent('0x434343...4343');
  const minted = within(named).getByRole('link', { name: '2023-11-15' });
  expect(minted).toHaveAttribute('href', '/mainnet/blocks/100');
  expect(minted).toHaveAttribute('title', '#100 · 2023-11-15 06:13:20 UTC+8');
});

it('counts genes from the pattern and says a decoder output is not drawn as one (V2)', async () => {
  const insights = {
    ...collectionInsightsProducer,
    currentPattern: {
      ...collectionInsightsProducer.currentPattern,
      steps: [
        ...collectionInsightsProducer.currentPattern.steps,
        {
          ...collectionInsightsProducer.currentPattern.steps[0],
          step: 1,
          description: { status: 'unsupported', reason: 'byte mapping is not defined' },
        },
      ],
    },
    genetics: {
      ...collectionInsightsProducer.genetics,
      traits: [
        ...collectionInsightsProducer.genetics.traits,
        {
          name: 'IMAGE',
          values: Array.from({ length: 8 }, (_, i) => ({
            value: `<svg xmlns='http://www.w3.org/2000/svg'><text>${i}</text></svg>`,
            count: 1,
          })),
        },
      ],
    },
  } as typeof collectionInsightsProducer;
  const { container } = render(
    <CollectionObjects id="collection" title="Genes" insights={insights} />
  );
  await screen.findByRole('table', { name: 'Newest objects' });
  const section = screen.getByRole('region', { name: 'Objects' });
  expect(section).toHaveTextContent('8 · 1 gene · how common each variant is');
  expect(container.querySelectorAll('[data-lane]')).toHaveLength(1);
  expect(container.querySelector('[data-lane="IMAGE"]')).toBeNull();
  expect(section).toHaveTextContent(
    'Not drawn: IMAGE, an output of the decoder that the pattern does not declare as a gene.'
  );
});

// Table-rows round, lane L2-4: a Newest row is one line — the object's name bright at 13 px (no
// second line of id: the Id column has it), owner, DNA gel, mint date; the rest in its title.
it('writes each Newest row on one line with the name bright and the rest in its title (L2-4)', async () => {
  const base = collectionGalleryFilteredProducer.data[0];
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([
      { ...base, number: '7' },
      { ...base, sporeId: `0x${'03'.repeat(32)}`, number: null, ownerAddress: null },
    ])
  );
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  const table = await screen.findByRole('table', { name: 'Newest objects' });
  const [, numbered, plain] = within(table).getAllByRole('row');
  const name = within(numbered).getByRole('link', { name: 'Genes #7' });
  expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]', 'block', 'truncate');
  // Without a number the id is the Id column's, not a second line under the name.
  const unnumbered = within(plain).getAllByRole('link')[0];
  expect(unnumbered).toHaveTextContent('Genes');
  expect(unnumbered).toHaveAccessibleName('Genes 0x03030303…0303');
  expect(unnumbered.querySelector('.sr-only')).toHaveTextContent('0x03030303…0303');
  expect(numbered).toHaveClass('py-1', 'min-h-11');
  expect(numbered).toHaveAttribute(
    'title',
    `Genes #7 · ${base.sporeId} · owner ${base.ownerAddress} · SECP256K1/blake160 · minted 2023-11-15 06:13:20 UTC+8 in block #100`
  );
  expect(plain.getAttribute('title')).toContain(`owner ${base.ownerLockHash}`);
});
