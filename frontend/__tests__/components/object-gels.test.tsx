import { useState } from 'react';
import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import { SequencingGel } from '@/components/ui/sequencing-gel';
import { AlleleGel } from '@/components/ui/allele-gel';
import { alleleLanes, geneOrder, genotypePoints, sequencingGenes } from '@/lib/dob-genes';
import { BITCOIN_COLORS, GENE_COLORS } from '@/lib/design-tokens';
import type { AlleleLane } from '@/lib/dob-genes';
import { nervape3xGenetics, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

const layout = sequencingGenes(nervape3xGenetics, 'Nervape 3x');
const lanes = alleleLanes(
  nervape3xInsights.genetics,
  geneOrder(nervape3xInsights.currentPattern),
  'Nervape 3x'
);

describe('SequencingGel (Object board, OB-7)', () => {
  // drawn 1:1 for its measured box; 896 px is the DNA plate at 1280, where the gene columns fit
  beforeEach(() => stubChartWidth(896));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws every hex digit in its own lane, top to bottom, with a byte ruler', () => {
    const { container } = render(
      <SequencingGel layout={layout} subject="Nervape 3x" active={null} onActive={() => {}} />
    );
    const bands = container.querySelectorAll('[data-nibble]');
    expect(bands).toHaveLength(106);
    // a9 → nibble 0 in lane a, nibble 1 in lane 9; lanes are 15 px wide from x = 38 (V3: the
    // ruler gutter gave 26 px to the gene columns so they fit the 896 px plate 1:1).
    expect(Number(bands[0].getAttribute('x'))).toBeCloseTo(38 + 10 * 15 + 2.5, 5);
    expect(Number(bands[1].getAttribute('x'))).toBeCloseTo(38 + 9 * 15 + 2.5, 5);
    expect(screen.getByText('byte')).toBeInTheDocument();
    expect(container.querySelectorAll('[data-ruler]').length).toBeGreaterThanOrEqual(7);
    expect(
      screen.getByRole('img', { name: /53-byte DNA as a sequencing gel: 16 lanes/ })
    ).toBeInTheDocument();
  });

  it('colours bands by owning gene and labels each gene beside its bands', () => {
    const { container } = render(
      <SequencingGel layout={layout} subject="Nervape 3x" active={null} onActive={() => {}} />
    );
    // byte 0 belongs to the Bitcoin pointer (copper), byte 32 (Color) is cyan.
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute(
      'fill',
      BITCOIN_COLORS.copper
    );
    expect(container.querySelector('[data-nibble="64"]')).toHaveAttribute(
      'fill',
      GENE_COLORS.cycle[0]
    );
    const bg = container.querySelector('[data-gene="prev.bg"]')!;
    expect(bg.textContent).toContain('bytes 0–31 · btcfs');
    expect(bg.textContent).toContain('a9461795…e0');
    expect(bg.textContent).toContain('Bitcoin inscription …79e0i0');
    const relief = container.querySelector('[data-gene="relief"]')!;
    expect(relief.textContent).toContain('byte 0 · 1 option');
    expect(relief.querySelector('[stroke-dasharray]')).not.toBeNull();
    expect(container.querySelector('[data-gene="Note"]')!.textContent).toContain('empty');
  });

  it('dims everything but the hovered gene and reports the gene up (three-way hover)', () => {
    const onActive = vi.fn();
    const { container, rerender } = render(
      <SequencingGel layout={layout} subject="Nervape 3x" active={null} onActive={onActive} />
    );
    fireEvent.mouseEnter(container.querySelector('[data-gene="Color"]')!);
    expect(onActive).toHaveBeenLastCalledWith('Color');
    rerender(
      <SequencingGel layout={layout} subject="Nervape 3x" active="Color" onActive={onActive} />
    );
    expect(container.querySelector('[data-nibble="64"]')).toHaveAttribute('opacity', '1');
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '0.15');
    expect(container.querySelector('[data-gene="Serial"]')).toHaveAttribute('opacity', '0.12');
    // a gene that only reads a shared byte lights that byte
    rerender(
      <SequencingGel layout={layout} subject="Nervape 3x" active="relief" onActive={onActive} />
    );
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '1');
    expect(container.querySelector('[data-nibble="2"]')).toHaveAttribute('opacity', '0.15');
  });
});

describe('AlleleGel (Collection board, OB-4 / OB-C2)', () => {
  // laid out for its measured box; 900 px is the object/collection column at 1280
  beforeEach(() => stubChartWidth(900));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('stacks one lane per gene in DNA order, dims conserved genes and counts variants', () => {
    const { container } = render(<AlleleGel lanes={lanes} total={90} subject="Nervape 3x" />);
    const laneEls = container.querySelectorAll('[data-lane]');
    expect([...laneEls].map((l) => l.getAttribute('data-lane'))).toEqual([
      'prev.type',
      'relief',
      'prev.bg',
      'Color',
      'Serial',
      'Note',
    ]);
    expect(container.querySelector('[data-lane="relief"] [data-allele]')).toHaveAttribute(
      'fill-opacity',
      '0.22'
    );
    expect(container.querySelector('[data-lane="Color"] [data-variants]')!.textContent).toBe('8');
    expect(
      container.querySelector('[data-lane="Color"] [data-allele="Water Veins"] title')!.textContent
    ).toBe('Color = Water Veins: 20 of 90 (22.22%)');
    expect(screen.getByText(/band thickness = share of all 90 objects/)).toBeInTheDocument();
  });

  it('draws a persistent genotype overlay for "this one" across every lane', () => {
    const own = genotypePoints(lanes, nervape3xGenetics.cachedVmTraits.traits);
    const { container } = render(
      <AlleleGel
        lanes={lanes}
        total={90}
        subject="Nervape 3x"
        overlays={[{ key: 'me', label: 'this one', points: own.points, persistent: true }]}
      />
    );
    const overlay = container.querySelector('[data-overlay="me"]')!;
    expect(overlay).toHaveAttribute('opacity', '1');
    expect(overlay.querySelectorAll('[data-tick]')).toHaveLength(6);
    expect(overlay.textContent).toContain('this one');
  });

  it('shows a row overlay only while that row is active, and links lanes to genes', () => {
    const onGene = vi.fn();
    const own = genotypePoints(lanes, nervape3xGenetics.cachedVmTraits.traits);
    const { container, rerender } = render(
      <AlleleGel
        lanes={lanes}
        total={90}
        subject="Nervape 3x"
        overlays={[{ key: 'row-0', label: '#1', points: own.points }]}
        activeOverlay={null}
        onGeneHover={onGene}
      />
    );
    expect(container.querySelector('[data-overlay="row-0"]')).toHaveAttribute('opacity', '0');
    fireEvent.mouseEnter(container.querySelector('[data-lane="Color"]')!);
    expect(onGene).toHaveBeenLastCalledWith('Color');
    rerender(
      <AlleleGel
        lanes={lanes}
        total={90}
        subject="Nervape 3x"
        overlays={[{ key: 'row-0', label: '#1', points: own.points }]}
        activeOverlay="row-0"
        activeGene="Color"
        onGeneHover={onGene}
      />
    );
    expect(container.querySelector('[data-overlay="row-0"]')).toHaveAttribute('opacity', '1');
    expect(container.querySelector('[data-lane="Serial"]')).toHaveAttribute('opacity', '0.2');
    expect(container.querySelector('[data-lane="Color"]')).toHaveAttribute('opacity', '1');
  });

  it('filters by clicking a band and marks the chosen allele', () => {
    const onBand = vi.fn();
    const { container, rerender } = render(
      <AlleleGel lanes={lanes} total={90} subject="Nervape 3x" onBandClick={onBand} />
    );
    fireEvent.click(container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')!);
    expect(onBand).toHaveBeenCalledWith('Color', 'Calm Blue');
    rerender(
      <AlleleGel
        lanes={lanes}
        total={90}
        subject="Nervape 3x"
        onBandClick={onBand}
        selected={{ Color: 'Calm Blue' }}
      />
    );
    expect(
      container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')
    ).toHaveAttribute('stroke', '#ffffff');
  });
});

/** The object page's DNA panel state: the gene lit across the trait list and both gels. */
function ControlledGel() {
  const [active, setActive] = useState<string | null>(null);
  return (
    <SequencingGel layout={layout} subject="Nervape 3x" active={active} onActive={setActive} />
  );
}

describe('SequencingGel at the measured width (V3)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws lanes and gene columns side by side, 1:1, where the columns fit', () => {
    stubChartWidth(896);
    const { container } = render(<ControlledGel />);
    const svg = container.querySelector('svg[role="img"]')!;
    expect(svg).toHaveAttribute('width', '896');
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 896 /);
    expect(svg.getAttribute('class')).not.toMatch(/min-w-/);
    expect(container.querySelector('.overflow-x-auto')).toBeNull();
    // the gene readings are SVG columns beside the bands, the value not cut
    expect(container.querySelector('[data-gene="prev.bg"]')!.textContent).toContain(
      'Bitcoin inscription …79e0i0'
    );
    // each column starts after the longest text of the one before: "bytes 51–52 · nervapeNote"
    // (25 characters of 11.5 px) ends before the raw column, which no longer collides with it
    const texts = Array.from(container.querySelectorAll('[data-gene="Note"] text'));
    const [bytesX, rawX] = [Number(texts[1].getAttribute('x')), Number(texts[2].getAttribute('x'))];
    expect(texts[1].textContent).toBe('bytes 51–52 · nervapeNote');
    expect(rawX - bytesX).toBeGreaterThanOrEqual(25 * 7.32);
    expect(container.querySelector('[data-gene-row]')).toBeNull();
    // the caption is HTML text under the SVG, 11 px
    const caption = screen.getByText(/^read top to bottom: each band is one hex digit/);
    expect(caption.closest('svg')).toBeNull();
    expect(caption).toHaveClass('text-[11px]');
  });

  it('keeps the lanes at 20 px and turns the gene columns into rows under the map at 358 px', () => {
    stubChartWidth(358);
    const { container } = render(<ControlledGel />);
    const svg = container.querySelector('svg[role="img"]')!;
    expect(svg).toHaveAttribute('width', '358');
    const x = (pos: number) =>
      Number(container.querySelector(`[data-nibble="${pos}"]`)!.getAttribute('x'));
    // a9: nibble 0 in lane a, nibble 1 in lane 9, one 20 px lane apart
    expect(x(0) - x(1)).toBe(20);
    for (const band of Array.from(svg.querySelectorAll('[data-nibble]')))
      expect(
        Number(band.getAttribute('x')) + Number(band.getAttribute('width'))
      ).toBeLessThanOrEqual(358);
    // no gene text inside the map; the dashed rules mark where Color, Serial and Note begin
    expect(svg.querySelector('[data-gene]')).toBeNull();
    expect(
      Array.from(svg.querySelectorAll('[data-boundary]')).map((b) =>
        b.getAttribute('data-boundary')
      )
    ).toEqual(['Color', 'Serial', 'Note']);
    const genes = screen.getByRole('list', { name: 'Genes' });
    // the longest reading (a9461795…e0 → Bitcoin inscription …79e0i0) does not fit a third
    // column at 358 px: each reading takes its own line under the name and bytes
    expect(genes).toHaveAttribute('data-rows', 'two-line');
    const rows = within(genes).getAllByRole('listitem');
    expect(rows.map((r) => r.getAttribute('data-gene-row'))).toEqual([
      'prev.type',
      'relief',
      'prev.bg',
      'Color',
      'Serial',
      'Note',
    ]);
    // name · bytes (and rule) · raw → value, the value never cut
    const bg = rows[2];
    for (const part of [
      'prev.bg',
      'bytes 0–31',
      'btcfs',
      'a9461795…e0',
      '→',
      'Bitcoin inscription …79e0i0',
    ])
      expect(bg).toHaveTextContent(part);
    expect(within(bg).getByText('Bitcoin inscription …79e0i0')).toHaveClass(
      '[overflow-wrap:anywhere]'
    );
    expect(rows[3]).toHaveTextContent('Color');
    expect(rows[3]).toHaveTextContent('byte 32');
    expect(rows[3]).toHaveTextContent('03');
    expect(rows[3]).toHaveTextContent('Nature Green');
    const caption = screen.getByText(/dashed rules mark the gene boundaries/);
    expect(caption.closest('svg')).toBeNull();
    expect(caption).toHaveClass('text-[11px]');
  });

  it('a tap on a gene row highlights its byte span and pins its reading; a second tap clears it', () => {
    stubChartWidth(358);
    const { container } = render(<ControlledGel />);
    const color = screen.getByRole('button', { name: /^Color, byte 32/ });
    fireEvent.click(color);
    const span = container.querySelector('[data-span="Color"]')!;
    expect(span).not.toBeNull();
    // byte 32 is nibbles 64 and 65: lit; byte 0 is dimmed
    expect(container.querySelector('[data-nibble="64"]')).toHaveAttribute('opacity', '1');
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '0.15');
    expect(color).toHaveAttribute('data-active', 'true');
    expect(color).toHaveAttribute('aria-pressed', 'true');
    const readout = screen.getByRole('status', { name: 'Gene readout' });
    expect(readout).toHaveTextContent(/^tapped Color · byte 32 · nervapeColor · 03 → Nature Green/);
    fireEvent.click(color);
    expect(container.querySelector('[data-span]')).toBeNull();
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '1');
    expect(readout).toHaveTextContent('Tap or hover a gene.');
  });

  it('keeps each gene on one line where its reading fits the third column (the 640 plate)', () => {
    stubChartWidth(556);
    render(<ControlledGel />);
    expect(screen.getByRole('list', { name: 'Genes' })).toHaveAttribute('data-rows', 'one-line');
  });

  it('refuses a box too narrow for 16 lanes of 14 px', () => {
    stubChartWidth(200);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<ControlledGel />)).toThrow(
      /Nervape 3x: a 200 px DNA map cannot hold 16 lanes/
    );
  });
});

/** A synthetic collection of `n` two-variant genes (the .cell deeds shape: 72 genes, 6 objects). */
const twoVariantLanes = (n: number): AlleleLane[] =>
  Array.from({ length: n }, (_, i) => ({
    gene: `gene${i}`,
    bands: [
      { value: 'a', label: 'a', count: 4, share: 4 / 6, isColour: false, colour: null },
      { value: 'b', label: 'b', count: 2, share: 2 / 6, isColour: false, colour: null },
    ],
    variants: 2,
    conserved: false,
    ordered: true,
  }));

describe('AlleleGel at the measured width (R3, responsive V2)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws 1:1 at 358 px with six lanes in two rows of three, "this one" kept on both rows', () => {
    stubChartWidth(358);
    const own = genotypePoints(lanes, nervape3xGenetics.cachedVmTraits.traits);
    const { container } = render(
      <AlleleGel
        lanes={lanes}
        total={90}
        subject="Nervape 3x"
        overlays={[{ key: 'me', label: 'this one', points: own.points, persistent: true }]}
      />
    );
    const svg = container.querySelector('svg[role="img"]')!;
    expect(svg).toHaveAttribute('width', '358');
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 358 /);
    expect(svg.getAttribute('class')).not.toMatch(/min-w-/);
    expect(container.querySelector('.overflow-x-auto')).toBeNull();
    const rows = Array.from(container.querySelectorAll('[data-gel-row]'));
    expect(
      rows.map((row) =>
        Array.from(row.querySelectorAll('[data-lane]')).map((l) => l.getAttribute('data-lane'))
      )
    ).toEqual([
      ['prev.type', 'relief', 'prev.bg'],
      ['Color', 'Serial', 'Note'],
    ]);
    // every band and every lane rect is inside the canvas and wider than zero
    for (const rect of Array.from(svg.querySelectorAll('rect'))) {
      const x = Number(rect.getAttribute('x'));
      const w = Number(rect.getAttribute('width'));
      expect(w).toBeGreaterThan(0);
      expect(x + w).toBeLessThanOrEqual(358);
    }
    // the genotype is marked on every lane and labelled on each row
    const overlay = container.querySelector('[data-overlay="me"]')!;
    expect(overlay.querySelectorAll('[data-tick]')).toHaveLength(6);
    expect(overlay.querySelectorAll('polyline')).toHaveLength(2);
    expect(Array.from(overlay.querySelectorAll('text')).map((t) => t.textContent)).toEqual([
      'this one',
      'this one',
    ]);
    // gene names are written across their lanes, never under 10 px
    const caption = container.querySelector('[data-lane="Color"] [data-gene-caption]')!;
    expect(caption).not.toHaveAttribute('transform');
    expect(Number(caption.getAttribute('font-size'))).toBeGreaterThanOrEqual(10);
  });

  it('a tap on a band pins its variant in the line under the gel; a second tap clears it (V2, R6)', () => {
    stubChartWidth(358);
    const onBand = vi.fn();
    const { container } = render(
      <AlleleGel lanes={lanes} total={90} subject="Nervape 3x" onBandClick={onBand} />
    );
    const svg = container.querySelector('svg[role="img"]')!;
    const readout = screen.getByRole('status', { name: 'Variant readout' });
    expect(svg.compareDocumentPosition(readout) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(readout).toHaveTextContent('Tap or hover a band.');
    const band = container.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')!;
    const calm = lanes.find((l) => l.gene === 'Color')!.bands.find((b) => b.value === 'Calm Blue')!;
    // a hover reads the band; a tap (emulated hover, then the click) pins it past the pointer
    fireEvent.mouseEnter(band);
    expect(readout).toHaveTextContent(`Color · ${calm.label} · ${calm.count} of 90 objects`);
    fireEvent.click(band);
    fireEvent.mouseLeave(band);
    expect(readout).toHaveTextContent(
      new RegExp(`^tapped Color · ${calm.label} · ${calm.count} of 90 objects`)
    );
    // on the collection page the same tap also filters by the variant
    expect(onBand).toHaveBeenCalledWith('Color', 'Calm Blue');
    fireEvent.mouseEnter(band);
    fireEvent.click(band);
    expect(readout).toHaveTextContent('Tap or hover a band.');
  });

  it('becomes a ranked gene list above 32 genes below md, and a gel from md up', () => {
    stubChartWidth(608);
    const many = twoVariantLanes(72);
    many[5] = {
      ...many[5],
      bands: [
        { value: 'x', label: 'x', count: 2, share: 2 / 6, isColour: false, colour: null },
        { value: 'y', label: 'y', count: 2, share: 2 / 6, isColour: false, colour: null },
        { value: 'z', label: 'z', count: 2, share: 2 / 6, isColour: false, colour: null },
      ],
      variants: 3,
    };
    many[9] = {
      ...many[9],
      bands: [{ ...many[9].bands[0], count: 6, share: 1 }],
      variants: 1,
      conserved: true,
    };
    const { container, unmount } = render(<AlleleGel lanes={many} total={6} subject="deeds" />);
    expect(container.querySelector('[data-lane]')).toBeNull();
    const table = screen.getByRole('table', {
      name: 'Genes ranked by how many variants they have',
    });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['gene', 'variants', 'most common']);
    const rows = within(table).getAllByRole('row').slice(1);
    // ten shown, the gene with the most variants first; the rest behind one control
    expect(rows).toHaveLength(10);
    expect(within(rows[0]).getAllByRole('cell')[0]).toHaveTextContent(/^gene5$/);
    expect(rows[0]).toHaveTextContent('2 of 6');
    const more = screen.getByRole('button', {
      name: /^62 more genes: 61 with 2 variants and 1 with 1 variant/,
    });
    fireEvent.click(more);
    expect(within(table).getAllByRole('row').slice(1)).toHaveLength(72);
    expect(screen.getByText(/genes ranked by how many variants/)).toBeInTheDocument();
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(736);
    const gel = render(<AlleleGel lanes={many} total={6} subject="deeds" />);
    expect(gel.container.querySelectorAll('[data-lane]')).toHaveLength(72);
    expect(gel.container.querySelectorAll('[data-gel-row]')).toHaveLength(2);
    expect(screen.queryByRole('table', { name: /Genes ranked/ })).toBeNull();
  });

  it('opens a gene in the list to show every variant, and filters by one', () => {
    stubChartWidth(358);
    const onBand = vi.fn();
    render(
      <AlleleGel
        lanes={twoVariantLanes(40)}
        total={6}
        subject="deeds"
        onBandClick={onBand}
        selected={{ gene0: 'b' }}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: /^gene0, 2 variants/ }));
    const variants = screen.getByRole('list', { name: 'gene0 variants' });
    expect(
      within(variants)
        .getAllByRole('listitem')
        .map((li) => li.textContent)
    ).toEqual(['a · 4 of 6', 'b · 2 of 6']);
    const b = within(variants).getByRole('button', { name: 'b · 2 of 6' });
    expect(b).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(within(variants).getByRole('button', { name: 'a · 4 of 6' }));
    expect(onBand).toHaveBeenCalledWith('gene0', 'a');
  });

  it('marks an object’s own variant per gene in the list form ("this one")', () => {
    stubChartWidth(358);
    const many = twoVariantLanes(40);
    render(
      <AlleleGel
        lanes={many}
        total={6}
        subject="deeds"
        overlays={[
          {
            key: 'me',
            label: 'this one',
            points: many.map((_, lane) => ({ lane, band: 1 })),
            persistent: true,
          },
        ]}
      />
    );
    const row = screen.getAllByRole('row')[1];
    expect(row).toHaveTextContent('▸ this one: b · 2 of 6');
  });

  it('throws on a box too narrow for one 12 px lane instead of drawing an empty gel', () => {
    stubChartWidth(80);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<AlleleGel lanes={lanes} total={90} subject="Nervape 3x" />)).toThrow(
      /Nervape 3x: a 80 px allele gel cannot hold a 12 px lane/
    );
  });
});
