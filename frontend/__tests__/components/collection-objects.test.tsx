// F2 group `objects` (round 2): the Collection board's Objects section (allele gel + Newest).
// The two tests of the retired 24-card gallery are kept here against the new list.
import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { CollectionObjects, DecodeCoverage } from '@/components/asset/collection-objects';
import { api, type CollectionInsights } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { nervape3xInsights, nervape3xOtherTraits } from '@/__tests__/fixtures/objects/nervape3x';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { phoneHeadPx, phoneLines, statedHeadPx, dottedItems } from '@/__tests__/utils/phone-row';
import { shortenAddress } from '@/lib/shorten';
import { PHONE_LINE_PX } from '@/lib/type-metrics';
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
      limit: 20,
      cursor: undefined,
      traits: { Face: 'Blue' },
    })
  );
  // re-pinned (lane c2-assets): the list is newest first, so the controls board's pager goes
  // "older →" (was "Next")
  fireEvent.click(await screen.findByRole('button', { name: 'older →' }));
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith('collection', {
      limit: 20,
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
// Readability B4 (re-pinned on purpose): the default lock is named on no row (R9), so the named
// row is a JoyID owner and the default-lock row says no lock.
it('names the owner lock kind and dates the mint in UTC+8 on each Newest row', async () => {
  const base = collectionGalleryFilteredProducer.data[0];
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([
      // 1700000000100 ms = 2023-11-14 22:13:20 UTC = 2023-11-15 06:13:20 UTC+8.
      // B4 fix round: each owner's address spells the lock the row names (the lock is read off it)
      {
        ...base,
        chainTime: '1700000000100',
        ownerAddress: seededAddress('joyid1', JOYID_CODE_HASH),
        lockCodeHash: JOYID_CODE_HASH,
        lockKind: 'JoyID',
      },
      {
        ...base,
        sporeId: `0x${'03'.repeat(32)}`,
        chainTime: '1700000000100',
        ownerAddress: seededAddress('unreg1', `0x${'43'.repeat(32)}`),
        lockCodeHash: `0x${'43'.repeat(32)}`,
        lockKind: null,
      },
      { ...base, sporeId: `0x${'04'.repeat(32)}`, chainTime: '1700000000100' },
    ])
  );
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  const table = await screen.findByRole('table', { name: 'Newest objects' });
  const [, named, unnamed, defaultLock] = within(table).getAllByRole('row');
  expect(named).toHaveTextContent('JoyID');
  expect(defaultLock).not.toHaveTextContent('SECP256K1/blake160');
  expect(defaultLock.querySelector('[data-lock-kind]')).toBeNull();
  // An unnamed lock shows its code hash, never a guessed kind.
  // U-09 (re-pinned): the row size
  expect(unnamed).toHaveTextContent('lock 0x43434343…434343');
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
  // C9 / Main.dc.html: the heading counts the genes that vary (the ones with a lane). Phase 5
  // (A14): the count and the genes are the objects command's args, no longer inside its word.
  const heading = within(section).getByRole('heading', {
    name: 'Objects 8 live · one gene varies',
  });
  expectPrompt(heading, 'Objects', '8 live · one gene varies');
  expect(heading).toHaveAttribute('id', 'h-objects');
  expect(container.querySelectorAll('[data-lane]')).toHaveLength(1);
  expect(container.querySelector('[data-lane="IMAGE"]')).toBeNull();
  expect(section).toHaveTextContent(
    'Not drawn: IMAGE, an output of the decoder that the pattern does not declare as a gene.'
  );
});

// Table-rows round, lane L2-4: a Newest row is one line — the object's name bright at 13 px; the
// rest in its title. Consistency round C11 (Main.dc.html): the row is object · the genes that
// vary · made of · owner · minted; the object is its number (else its short id), never the
// collection's name again; its id and DNA are in the title.
// B4 second fix round (review-rules-fixes-logic SHOULD 3, the coordinator's item 5): a row's title
// writes its owner's lock as the row's tag does, from the one decision: nothing for the default
// lock, its code hash and hash type for a lock on the default code hash by data.
it('titles a Newest row with its owner’s lock as its tag writes it', async () => {
  const base = collectionGalleryFilteredProducer.data[0];
  const byData =
    'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl';
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([
      { ...base, ownerAddress: byData },
      { ...base, sporeId: `0x${'03'.repeat(32)}` },
    ])
  );
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  const table = await screen.findByRole('table', { name: 'Newest objects' });
  const [, byDataRow, defaultRow] = within(table).getAllByRole('row');
  expect(byDataRow.getAttribute('title')).toContain(
    `owner ${byData} · lock 0x9bd7e06f…a3cce8 by data · minted`
  );
  expect(defaultRow.getAttribute('title')).toContain(`owner ${base.ownerAddress} · minted`);
});

// B4 second fix round (review-rules-fixes-logic NOTE 8, the coordinator's item 14, ruling D7): a
// row whose owner address spells another lock than it says is refused by the lock decision, and
// the refusal fails the Newest list in its place, never the page.
it('says in its place that it cannot read the newest objects when an owner’s lock is refused', async () => {
  const base = collectionGalleryFilteredProducer.data[0];
  const joy = seededAddress('joyown', JOYID_CODE_HASH);
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([{ ...base, ownerAddress: joy }])
  );
  render(
    <main>
      <h1>The collection page</h1>
      <CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />
    </main>
  );
  // third round (re-pinned on purpose, the coordinator's item 4): said where (the collection and
  // its page), the section's pager outside the failed part
  const alert = await screen.findByText(
    /^Could not draw the newest objects \(collection collection, page 1\): /
  );
  expect(alert).toHaveTextContent(`address ${joy} spells the lock code hash ${JOYID_CODE_HASH}`);
  expect(screen.getByRole('heading', { name: 'The collection page' })).toBeInTheDocument();
});

it('writes each Newest row on one line with the name bright and the rest in its title (L2-4, C11)', async () => {
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
  const name = within(numbered).getByRole('link', { name: '#7' });
  expect(name).toHaveClass('text-body', 'text-text-bright', 'leading-row', 'truncate');
  // Without a number the object is its short id.
  const unnumbered = within(plain).getAllByRole('link')[0];
  expect(unnumbered).toHaveAccessibleName('0x03030303…030303');
  expect(numbered).toHaveClass('py-1', 'min-h-[41px]');
  expect(
    within(table)
      .getAllByRole('columnheader')
      .map((h) => h.textContent)
  ).toEqual(['Object', 'Face', 'Made of', 'Owner', 'Minted']);
  const own = base.traits.find((t) => t.name === 'Face')!;
  expect(numbered).toHaveAttribute(
    'title',
    // B4 second fix round (re-pinned on purpose): the owner's lock as its tag writes it, nothing
    // for the default lock (it wrote the API's name, 'SECP256K1/blake160')
    `Genes #7 · ${base.sporeId} · Face ${own.value} · owner ${base.ownerAddress} · minted 2023-11-15 06:13:20 UTC+8 in block #100 · DNA ${base.dnaHex}`
  );
  expect(plain.getAttribute('title')).toContain(`owner ${base.ownerLockHash}`);
});

// Consistency round (Main.dc.html) + RESPONSIVE_LAYOUT rule 4: the gel sits beside the table only
// when it can be drawn as a gel in 440 px; a collection with more lanes than a 440 px gel holds
// (above 32, which needs 736 px) puts the gel above the table at full width instead of turning it
// into the ranked text list.
it('puts a many-gene gel above the table at full width, a few-gene gel beside it', async () => {
  const traits = Array.from({ length: 33 }, (_, i) => ({
    name: `g${i}`,
    values: [
      { value: 'a', count: 4 },
      { value: 'b', count: 4 },
    ],
  }));
  const many = {
    ...collectionInsightsProducer,
    currentPattern: { status: 'notDob', steps: [] },
    genetics: { ...collectionInsightsProducer.genetics, traits },
  } as CollectionInsights;
  const { unmount } = render(<CollectionObjects id="collection" title="Genes" insights={many} />);
  await screen.findByRole('table', { name: 'Newest objects' });
  const columns = (section: HTMLElement) => section.querySelector('[data-role="objects-columns"]')!;
  expect(columns(screen.getByRole('region', { name: 'Objects' })).className).not.toMatch(
    /xl:grid-cols-\[440px/
  );
  expect(screen.getByRole('img', { name: /allele gel of 8\u00a0objects/ })).toBeInTheDocument();
  unmount();
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  await screen.findByRole('table', { name: 'Newest objects' });
  expect(columns(screen.getByRole('region', { name: 'Objects' }))).toHaveClass(
    'xl:grid-cols-[440px_minmax(0,1fr)]'
  );
});

// Consistency round C11: the objects table pages with the one pager wording, not its own
// "N objects, 20 per page" line. Re-pinned (lane c2-assets): the one wording is the controls
// board's pager, and a list newest first is a list in time — its order, "← newer", "page 1 of 65",
// "older →" (was "Showing 1–20 of 1,298 objects · 20 per page" and "Page 1 of 65"); the count
// stays the list's own line. The page states the page size it served, 20, as the API echoes the
// request (the fixture said 50).
it('pages the objects with the shared pager wording (C11)', async () => {
  const base = collectionGalleryFilteredProducer.data[0];
  vi.mocked(api.getCollectionGallery).mockResolvedValue({
    ...emptyPage(
      Array.from({ length: 20 }, (_, i) => ({
        ...base,
        sporeId: `0x${(i + 16).toString(16).padStart(2, '0').repeat(32)}`,
        number: String(i + 1),
      }))
    ),
    limit: 20,
    total: 1298,
    hasMore: true,
    nextCursor: 'older',
  });
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  await screen.findByRole('table', { name: 'Newest objects' });
  const pager = screen.getByRole('navigation', { name: 'objects' });
  expect(pager).toHaveTextContent(/^newest first← newerpage 1 of 65older →$/);
  expect(within(pager).queryByRole('button', { name: '← newer' })).toBeNull();
  expect(within(pager).getByRole('button', { name: 'older →' })).toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Objects' })).toHaveTextContent('1,298 objects');
  expect(screen.queryByText(/objects, 20 per page/)).toBeNull();
});

// Lane A follow-up 1: the gene columns clipped "…bee9i0 · 3 of 1,298" to "…bee9i0 · 3 o…" and
// "0 · 1,139" to "0 · 1,13…". A gene cell never cuts its share or a number: the share does not
// shrink, the value is cut only when it is words, and each gene column is at least as wide (in ch,
// the font is monospaced) as the swatch, a value's shown part and the widest share of its lane.
describe('gene cells keep their share whole (lane A follow-up)', () => {
  const columnMinimums = (table: HTMLElement) =>
    [...table.style.getPropertyValue('--object-cols').matchAll(/minmax\((\d+)ch/g)].map((m) =>
      Number(m[1])
    );
  const checkCells = (table: HTMLElement) => {
    const mins = columnMinimums(table);
    const genes = within(table)
      .getAllByRole('columnheader')
      .map((h) => h.textContent)
      .filter((t) => !['Object', 'Made of', 'Owner', 'Minted'].includes(t ?? ''));
    expect(genes.length).toBeGreaterThan(0);
    expect(mins).toHaveLength(genes.length);
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      const cells = [...row.querySelectorAll<HTMLElement>('[data-gene-cell]')];
      expect(cells).toHaveLength(mins.length);
      cells.forEach((cell, i) => {
        expect(cell).not.toHaveClass('truncate');
        const share = cell.querySelector('[data-role="gene-share"]')!;
        expect(share).toHaveClass('shrink-0', 'whitespace-nowrap');
        expect(share.closest('.truncate')).toBeNull();
        const value = cell.querySelector('[data-role="gene-value"]')!;
        const text = value.textContent!;
        const numeric = /^[\d,.]+$/.test(text);
        if (numeric) expect(value).not.toHaveClass('truncate');
        const need =
          3 + (numeric ? text.length : Math.min(text.length, 8)) + share.textContent!.length;
        expect(need).toBeLessThanOrEqual(mins[i]);
      });
    }
  };
  it('on a cluster with a Bitcoin pointer and a colour gene', async () => {
    const base = collectionGalleryFilteredProducer.data[0];
    vi.mocked(api.getCollectionGallery).mockResolvedValue(
      emptyPage(
        Object.entries(nervape3xOtherTraits)
          .slice(0, 6)
          .map(([sporeId, traits]) => ({ ...base, sporeId, traits: [...traits] }))
      )
    );
    render(<CollectionObjects id="collection" title="Nervape 3x" insights={nervape3xInsights} />);
    checkCells((await screen.findByRole('table', { name: 'Newest objects' })) as HTMLElement);
  });
  it('on a numeric gene whose variant is carried by four-digit counts', async () => {
    const values = [
      { value: '0', count: 1139 },
      { value: '1', count: 159 },
    ];
    const insights = {
      ...collectionInsightsProducer,
      currentPattern: { status: 'notDob', steps: [] },
      genetics: {
        ...collectionInsightsProducer.genetics,
        liveItems: 1298,
        eligibleItems: 1298,
        decodedItems: 1298,
        traits: [
          { name: 'prev.view', values },
          {
            name: 'prev.bg',
            values: Array.from({ length: 52 }, (_, i) => ({
              value: `btcfs://${'b4'.repeat(32)}i${i}`,
              count: i === 0 ? 1247 : 1,
            })),
          },
        ],
      },
    } as CollectionInsights;
    const base = collectionGalleryFilteredProducer.data[0];
    vi.mocked(api.getCollectionGallery).mockResolvedValue(
      emptyPage([
        {
          ...base,
          traits: [
            { name: 'prev.view', value: '0' },
            { name: 'prev.bg', value: `btcfs://${'b4'.repeat(32)}i0` },
          ],
        },
      ])
    );
    render(<CollectionObjects id="collection" title="Nervape Headwear" insights={insights} />);
    const table = (await screen.findByRole('table', { name: 'Newest objects' })) as HTMLElement;
    checkCells(table);
    // The count is the visible share; "of 1,298" (the same for every cell) is in the cell's title,
    // so the object and owner columns keep their room.
    const view = table.querySelectorAll('[data-gene-cell]')[0];
    expect(view).toHaveTextContent(/^0 · 1,139$/);
    expect(view).toHaveAttribute('title', 'View = 0 · 1,139 of 1,298\u00a0objects');
    expect(within(table).getByRole('columnheader', { name: 'View' })).toHaveAttribute(
      'title',
      'prev.view · beside each value, how many of the 1,298\u00a0objects carry it'
    );
    // One line per object from md up; below it every column stays, in pairs.
    expect(table.querySelectorAll('[role="row"]')[1]).toHaveClass(
      'grid-cols-[minmax(0,1fr)_auto]',
      'md:grid-cols-[var(--object-cols)]'
    );
  });
});

// Readability B7, the third round (review of the second fixes): the dropped-genes caption stands
// in a 400 px box on the collection page, and the box is drawn exactly when the caption is: an
// empty box in the gel's `space-y-3` column added a 12 px gap below xl where no gene is dropped.
describe('the dropped-genes caption’s box', () => {
  const gelColumn = (container: HTMLElement) =>
    container.querySelector('[data-role="objects-columns"] > div') as HTMLElement;

  it('draws no box when no gene is dropped, nor any empty element in the gel’s column', async () => {
    const { container } = render(
      <CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />
    );
    await screen.findByRole('table', { name: 'Newest objects' });
    const column = gelColumn(container);
    expect(column).toHaveClass('space-y-3');
    expect(container.querySelector('.max-w-\\[400px\\]')).toBeNull();
    for (const child of Array.from(column.children))
      expect(child.childNodes.length).toBeGreaterThan(0);
  });

  it('draws the caption inside its 400 px box, on the measure, when a gene is dropped', async () => {
    // a second gene the pattern declares, one value on every object: it gets no lane (C9)
    const pattern = collectionInsightsProducer.currentPattern;
    const step = pattern.steps[0];
    const face = step.description.genes[0];
    const insights = {
      ...collectionInsightsProducer,
      currentPattern: {
        ...pattern,
        steps: [
          {
            ...step,
            description: {
              ...step.description,
              genes: [
                face,
                { ...face, index: 1, name: 'Hat', byteOffset: '1', ruleArguments: ['Cap'] },
              ],
            },
          },
        ],
      },
      genetics: {
        ...collectionInsightsProducer.genetics,
        traits: [
          ...collectionInsightsProducer.genetics.traits,
          { name: 'Hat', values: [{ value: 'Cap', count: 8 }] },
        ],
      },
    } as CollectionInsights;
    const { container } = render(
      <CollectionObjects id="collection" title="Genes" insights={insights} />
    );
    await screen.findByRole('table', { name: 'Newest objects' });
    const boxes = gelColumn(container).querySelectorAll('.max-w-\\[400px\\]');
    expect(boxes).toHaveLength(1);
    expect(boxes[0].children).toHaveLength(1);
    const caption = boxes[0].firstElementChild!;
    expect(caption).toHaveAttribute('data-role', 'caption');
    expect(caption).toHaveClass('max-w-measure');
    expect(caption).toHaveTextContent(/Hat/);
  });
});

// Lane D4 follow-up (the coordinator's item 1; PB-10): below md a cluster's newest objects were the
// table's grid in two columns, its cells flowing in pairs — the minting day and the block with no
// label, the genes split over two lines, the owner's lock cut to "…" in its tag. Each object is the
// phone-rows board's item row now: its name alone on line 1 (it may wrap, it is never cut); who holds
// it on line 2, its lock in plain words where it is not the default lock, the composition chip held
// at the line's right edge; quiet on line 3, the genes the grid shows as columns, each named by its
// gene, and where and when it was minted, named. The share of each gene (how many objects carry it)
// is the gel's above the list and the cell's title in the table; the object's page has every gene.
describe('a cluster’s newest objects on a phone (lane D4, PB-10)', () => {
  const base = collectionGalleryFilteredProducer.data[0];
  const joy = seededAddress('joyid1', JOYID_CODE_HASH);

  it('is the board’s item row below md: name · holder and chip · genes and the mint, named', async () => {
    atViewportWidth(390);
    vi.mocked(api.getCollectionGallery).mockResolvedValue(
      emptyPage([
        {
          ...base,
          chainTime: '1700000000100',
          ownerAddress: joy,
          lockCodeHash: JOYID_CODE_HASH,
          lockKind: 'JoyID',
          mediaProfile: { ...base.mediaProfile, tier: 'pure_ckb' },
        },
        {
          ...base,
          sporeId: `0x${'04'.repeat(32)}`,
          number: null,
          ownerLockHash: null,
          ownerAddress: null,
          // no media profile: the grid's "not recorded"
          mediaProfile: null,
        },
      ])
    );
    render(
      <CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />
    );
    const [held, ownerless] = await screen.findAllByTestId('cluster-object-row-mobile');
    expect(screen.queryByRole('table', { name: 'Newest objects' })).toBeNull();
    expect(phoneLines(held).map((l) => [l.key, l.tone, l.left, l.right])).toEqual([
      ['name', 'head', '#18,446,744,073,709,551,614', null],
      ['holder', 'body', `held by ${shortenAddress(joy, 'row')} JoyID`, 'Pure CKB'],
      ['minted', 'quiet', 'Face Blue · minted at #100 on 2023-11-15', null],
    ]);
    // the chip holds the holder's line's right edge; the lock is words, never a tag cut to "…"
    // re-pinned on purpose (lane p-assets, review of C and D logic SHOULD 4): a held line wraps by
    // its own rule, the chip under the left only where it and the left's widest item cannot share
    // the line (its left part's basis nothing, its least width its min-content; it was flex-nowrap)
    expect(held.querySelector('[data-line="holder"] > [data-part="left"]')).toHaveClass(
      'basis-0',
      'min-w-min'
    );
    expect(held.querySelector('[data-line="holder"] [data-row-tag]')).toBeNull();
    expect(dottedItems(held.querySelector('[data-line="minted"]')!)).toEqual([
      'Face Blue',
      'minted at #100 on 2023-11-15',
    ]);
    const labels = Array.from(held.querySelectorAll('[data-role="figure-label"]'));
    expect(labels.map((l) => l.textContent)).toEqual(['held by', 'Face', 'minted at', 'on']);
    expect(phoneLines(ownerless).map((l) => [l.key, l.left, l.right])).toEqual([
      ['name', '0x04040404…040404', null],
      ['holder', 'No live owner', 'not recorded'],
      ['minted', 'Face Blue · minted at #100 on 2023-11-15', null],
    ]);
    // line 1 is the name alone at its widest: the mark and its gap and a u64 object number
    expect(statedHeadPx(held)).toBe(246.8);
    expect(statedHeadPx(held)).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
    expect(phoneHeadPx(held, 360)).toBeLessThanOrEqual(statedHeadPx(held));
  });

  it('keeps the table from md', async () => {
    atViewportWidth(1024);
    vi.mocked(api.getCollectionGallery).mockResolvedValue(emptyPage([base]));
    render(
      <CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />
    );
    await screen.findByRole('table', { name: 'Newest objects' });
    expect(screen.queryByTestId('cluster-object-row-mobile')).toBeNull();
  });
});
