import { useState } from 'react';
import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import { SequencingGel } from '@/components/ui/sequencing-gel';
import { AlleleGel } from '@/components/ui/allele-gel';
import { alleleLanes, geneOrder, genotypePoints, sequencingGenes } from '@/lib/dob-genes';
import { BITCOIN_COLORS, GENE_COLORS } from '@/lib/design-tokens';
import type { AlleleLane } from '@/lib/dob-genes';
import { nervape3xGenetics, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { fullText } from '@/__tests__/utils/full-text';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';
import { allTextLines, expectApart, expectInside, viewBox } from '@/__tests__/utils/svg-text-boxes';
import { fontPx, textPx } from '@/lib/type-metrics';

const layout = sequencingGenes(nervape3xGenetics, 'Nervape 3x');
const lanes = alleleLanes(
  nervape3xInsights.genetics,
  geneOrder(nervape3xInsights.currentPattern),
  'Nervape 3x'
);

describe('SequencingGel (Object board, OB-7)', () => {
  // drawn 1:1 for its measured box; 896 px is the DNA plate at 1280, where the gene columns fit.
  // The review's item B (the lead's ruling): 8da9b2b5 moved this to 937 px, a width no viewport
  // gives, when the inscription reading grew to 33 characters; the reading is now
  // "inscription a94617…79e0i0" (25) and the plate is 896 again
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
    // a9 → nibble 0 in lane a, nibble 1 in lane 9; lanes are 15 px wide from x = 42 (V3: the
    // ruler gutter gave 26 px to the gene columns so they fit the 896 px plate 1:1; re-pinned with
    // readability F3: the gutter holds its "byte", 1 + 29 + 12 = 42, was 38)
    expect(Number(bands[0].getAttribute('x'))).toBeCloseTo(42 + 10 * 15 + 2.5, 5);
    expect(Number(bands[1].getAttribute('x'))).toBeCloseTo(42 + 9 * 15 + 2.5, 5);
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
    expect(bg.textContent).toContain('a94617…79e0');
    expect(bg.textContent).toContain('inscription a94617…79e0i0');
    const relief = container.querySelector('[data-gene="relief"]')!;
    expect(relief.textContent).toContain('byte 0 · 1 option');
    expect(relief.querySelector('[stroke-dasharray]')).not.toBeNull();
    expect(container.querySelector('[data-gene="Note"]')!.textContent).toContain('empty');
  });

  // Lane E2 (ruling R4): the raw bytes beside a gene are words in the gene's own ink at full
  // strength; at fill-opacity .85 the copper read 3.92:1, the indigo 4.24:1 and the dim 3.77:1 on
  // the day page (4.12:1 the dim by night), each ≥ 4.57:1 at full strength.
  it('draws each gene’s raw bytes in the gene ink at full strength, no word thinned at rest', () => {
    const { container } = render(
      <SequencingGel layout={layout} subject="Nervape 3x" active={null} onActive={() => {}} />
    );
    const texts = [...container.querySelectorAll('[data-gene="prev.bg"] text')];
    // U-09 (re-pinned): the raw bytes take the tight size of the one rule, 6…4
    const raw = texts.find((t) => t.textContent === 'a94617…79e0')!;
    expect(raw).toHaveAttribute('fill', BITCOIN_COLORS.copper);
    expect(raw).not.toHaveAttribute('fill-opacity');
    const thinned = [...container.querySelectorAll('text, tspan')].filter(
      (t) => t.hasAttribute('fill-opacity') || t.hasAttribute('opacity')
    );
    expect(thinned.map((t) => t.textContent)).toEqual([]);
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
    // the count is one unit (B3 round 4, task 3): the caption is read across its span
    expect(
      screen.getByText(fullText(/band thickness = share of all 90 objects/))
    ).toBeInTheDocument();
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

  it('draws its wells, bands and the "this one" outline square-cornered (U-02, R1)', () => {
    const own = genotypePoints(lanes, nervape3xGenetics.cachedVmTraits.traits);
    const { container } = render(
      <AlleleGel
        lanes={lanes}
        total={90}
        subject="Nervape 3x"
        overlays={[{ key: 'me', label: 'this one', points: own.points, persistent: true }]}
      />
    );
    expect(container.querySelectorAll('rect[data-allele]').length).toBeGreaterThan(6);
    expect(container.querySelectorAll('rect[data-tick]')).toHaveLength(6);
    expect(roundedRects(container)).toEqual([]);
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
    ).toHaveAttribute('stroke', 'var(--c-gel-overlay)');
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

  // The review's item B: the wide gel holds at the 896 px plate (1280) for this DNA with a value
  // at the full budget. Its columns before the value end at 692.2 px: the gene columns from 304
  // (38 + 16 × 15 + 10 + 16), the longest name ("prev.type", 9 × 7.8 + 10), the longest bytes ·
  // rule ("bytes 51–52 · nervapeNote", 25 × 7.5 + 12), the raw at the tight size (11 × 7.5 + 8),
  // the arrow's 16 and the 2 px end. 896 − 692.2 = 203.8 px holds 26 characters of 7.8 (202.8)
  // and not 27 (210.6): the budget is 26. A value of 40 characters is cut to it and still fits.
  // Re-pinned (readability R4): the gene text is 13 and 12.5 px at 0.6 em (was 12 and 11.5 px at
  // a measured 0.636 em), so the budget is 26 (was 27). Re-pinned (readability F3): the ruler's
  // gutter holds its "byte" (42 px, was 38), the gene columns start at 308 and the value at 696.2,
  // and 896 − 696.2 = 199.8 px holds 25 characters (195) and not 26 (202.8): the budget is 25.
  it('draws the wide gel at the 896 px plate with a value cut to the full budget', () => {
    stubChartWidth(896);
    const long = 'N'.repeat(40);
    const full = {
      ...layout,
      genes: layout.genes.map((g) => (g.name === 'Serial' ? { ...g, value: long } : g)),
    };
    const { container } = render(
      <SequencingGel layout={full} subject="Nervape 3x" active={null} onActive={() => {}} />
    );
    expect(screen.queryByRole('list', { name: 'Genes' })).toBeNull();
    // the drawn words (the group's <title> keeps the whole value for a reader)
    const words = Array.from(container.querySelectorAll('[data-gene="Serial"] text')).map(
      (t) => t.textContent
    );
    expect(words).toContain(`${'N'.repeat(24)}…`);
    expect(words.some((w) => w!.includes('N'.repeat(25)))).toBe(false);
    expect(container.querySelector('[data-gene="prev.bg"]')!.textContent).toContain(
      'inscription a94617…79e0i0'
    );
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
      'inscription a94617…79e0i0'
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
    expect(caption).toHaveClass('text-meta');
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
    // the longest reading (a94617…79e0 → inscription a94617…79e0i0) does not fit a third
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
      'a94617…79e0',
      '→',
      'inscription a94617…79e0i0',
    ])
      expect(bg).toHaveTextContent(part);
    // the value wraps anywhere as third-party text does, its shortened id one unit inside it (B3
    // round 2, re-pinned on purpose: the id is a NoBreak of its own, so the value is two nodes)
    const id = within(bg).getByText('a94617…79e0i0', { selector: '[data-nobreak]' });
    const value = id.parentElement as HTMLElement;
    expect(value).toHaveTextContent(/^inscription a94617…79e0i0$/);
    expect(value).toHaveClass('[overflow-wrap:anywhere]');
    expect(rows[3]).toHaveTextContent('Color');
    expect(rows[3]).toHaveTextContent('byte 32');
    expect(rows[3]).toHaveTextContent('03');
    expect(rows[3]).toHaveTextContent('Nature Green');
    const caption = screen.getByText(/dashed rules mark the gene boundaries/);
    expect(caption.closest('svg')).toBeNull();
    expect(caption).toHaveClass('text-meta');
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
    expect(readout).toHaveTextContent(
      /^selected Color · byte 32 · nervapeColor · 03 → Nature Green/
    );
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

  it.each([896, 358])(
    'draws its wells, bands, gene spans and field square at %i px, a lit gene too (U-02, R1)',
    (width) => {
      stubChartWidth(width);
      const { container } = render(
        <SequencingGel layout={layout} subject="Nervape 3x" active="Color" onActive={() => {}} />
      );
      expect(container.querySelectorAll('rect[data-nibble]')).toHaveLength(106);
      expect(rects(container).length).toBeGreaterThan(106);
      expect(roundedRects(container)).toEqual([]);
    }
  );

  it('refuses a box too narrow for 16 lanes of 14 px', () => {
    stubChartWidth(200);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<ControlledGel />)).toThrow(
      /Nervape 3x: a 200 px DNA map cannot hold 16 lanes/
    );
  });

  // Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
  // of the DNA map stands whole inside the svg and touches no other word — the lane digits, the
  // byte ruler, a gene's reading row beside the lanes (each row a tight line at the head size, so
  // two genes' rows stand apart) — measured by the type modules (0.6 em a glyph; a tight line over
  // its baseline, its descent under it); and in the narrow form each gene's name and its bytes
  // stand whole in their columns of the rows under the map. The real case is Nervape 3x's DNA, at
  // the DNA plate at 1280 (896) and wider (1232), at 640 (608) and on phones (358, 326).
  it.each([1232, 896, 608, 358, 326])(
    'writes every word whole and apart at %i px (readability F3)',
    (width) => {
      stubChartWidth(width);
      const { container } = render(<ControlledGel />);
      const svg = container.querySelector('svg[role="img"]')!;
      const lines = allTextLines(svg);
      expect(lines.length).toBeGreaterThan(16);
      for (const line of lines) expectInside(line, viewBox(svg), 'the map', 'flush');
      expectApart(lines, `the map at ${width}`);
      const list = screen.queryByRole('list', { name: 'Genes' });
      if (list?.getAttribute('data-rows') !== 'one-line') return;
      // the one-line rows under a narrow map: the name's column holds its name (the small step), the
      // bytes' column its bytes and its rule (the meta step), each on a line of its own; the columns
      // are read from the row's grid (a class, or a style where they are computed)
      const row = within(list).getAllByRole('listitem')[0].firstElementChild as HTMLElement;
      const grid =
        row.style.gridTemplateColumns || /grid-cols-\[([^\]]+)\]/.exec(row.className)![1];
      const [nameCol, bytesCol] = /(\d+)px[ _](\d+)px/.exec(grid)!.slice(1).map(Number);
      for (const g of layout.genes) {
        expect(textPx(g.name.length, fontPx('small')), `${g.name}`).toBeLessThanOrEqual(nameCol);
        const bytes =
          g.length === 1 ? `byte ${g.offset}` : `bytes ${g.offset}–${g.offset + g.length - 1}`;
        for (const words of [bytes, g.rule])
          expect(textPx(words.length, fontPx('meta')), words).toBeLessThanOrEqual(bytesCol);
      }
    }
  );
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
    // Re-pinned with PB-04 (UI review 2026-11-14): the genotype is joined side to side between
    // neighbouring lanes (two links in each row of three), not by a polyline through the bands.
    expect(overlay.querySelectorAll('[data-link]')).toHaveLength(4);
    expect(overlay.querySelectorAll('polyline')).toHaveLength(0);
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
      new RegExp(`^selected Color · ${calm.label} · ${calm.count} of 90 objects`)
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
      name: /^62\smore\sgenes: 61 with 2\svariants and 1 with 1\svariant/,
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
    fireEvent.click(screen.getByRole('button', { name: /^gene0, 2\svariants/ }));
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

  // Re-pinned with PB-04: with no tag always shown there is no left margin, so the narrowest box
  // that still holds a 12 px lane is 34 px (the 22 px right edge and one lane), not 96. Re-pinned
  // with readability F3: the floor is 16 px (a turned 12 px caption's line and a pixel), so 38 px.
  it('throws on a box too narrow for one lane at the floor instead of drawing an empty gel', () => {
    stubChartWidth(30);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<AlleleGel lanes={lanes} total={90} subject="Nervape 3x" />)).toThrow(
      /Nervape 3x: a 30 px allele gel cannot hold a 16 px lane/
    );
  });
});

/*
 * Review F-B (logic finding 5): a gene's name, its rule and its trait value are the collection
 * creator's text, and the gels measured them at 0.6 em a UTF-16 unit. No face the site serves
 * holds a CJK character or an emoji: the browser draws them from a fallback face, 1.2 em for
 * CJK in the monospace face the owner's machine falls back to and 1.245 em for its emoji. Such a
 * text is measured as an upper bound, 1.25 em a grapheme that is not in the served faces' Latin
 * subset, and cut at whole graphemes. The bounds here are literal arithmetic over the characters
 * of each test string: at the head size (13 px) a Latin glyph is 7.8 px and any other 16.25.
 */
const LATIN = 0.6;
const WIDE = 1.25;
/** The bound of a text of `latin` Latin glyphs and `wide` other graphemes at `px`. */
const bound = (latin: number, wide: number, px: number) => latin * LATIN * px + wide * WIDE * px;
/** A text with a high surrogate not followed by its low half (a cut through an emoji). */
const LONE_SURROGATE = /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;

describe('the gels measure the creator’s text as an upper bound (review F-B, logic finding 5)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** Nervape 3x's DNA with the Color gene renamed and given a value, and Serial given one. */
  const creators = (color: { name?: string; rule?: string; value?: string }, serial?: string) => ({
    ...layout,
    genes: layout.genes.map((g) =>
      g.name === 'Color'
        ? { ...g, ...color }
        : g.name === 'Serial' && serial !== undefined
          ? { ...g, value: serial }
          : g
    ),
  });

  it('starts the wide gel’s bytes column after a CJK name’s bound', () => {
    // at 1232 px (at the 896 px plate the wider name column leaves too little for the widest
    // value, and the gel takes its narrow form)
    stubChartWidth(1232);
    // "颜色和图案", 5 CJK characters: 81.25 px at 13 px (it was counted 39)
    const { container } = render(
      <SequencingGel
        layout={creators({ name: '颜色和图案' })}
        subject="Nervape 3x"
        active={null}
        onActive={() => {}}
      />
    );
    const texts = [...container.querySelectorAll('[data-gene="颜色和图案"] text')];
    expect(texts[0].textContent).toBe('颜色和图案');
    const [nameX, bytesX] = [0, 1].map((i) => Number(texts[i].getAttribute('x')));
    expect(bytesX - nameX).toBeGreaterThanOrEqual(bound(0, 5, 13) + 10);
  });

  it('cuts a CJK value to its room and keeps it inside the svg', () => {
    stubChartWidth(896);
    // 20 CJK characters (325 px) were 20 code units, under the 25 of the budget, so drawn whole
    // from x 696.2 to 1021; the room is 25 Latin glyphs, 195 px: 11 of them and the ellipsis
    const { container } = render(
      <SequencingGel
        layout={creators({ value: '猫'.repeat(20) })}
        subject="Nervape 3x"
        active={null}
        onActive={() => {}}
      />
    );
    const value = [...container.querySelectorAll('[data-gene="Color"] text')].at(-1)!;
    expect(value.textContent).toBe(`${'猫'.repeat(11)}…`);
    expect(Number(value.getAttribute('x')) + bound(1, 11, 13)).toBeLessThanOrEqual(896);
  });

  it('never cuts through an emoji: a value ends at a whole grapheme and the one ellipsis', () => {
    stubChartWidth(896);
    // 23 Latin glyphs, then a family emoji (a ZWJ sequence of 8 UTF-16 units): the cut at 24
    // units kept its first surrogate alone; the emoji does not fit what is left of 195 px
    const { container } = render(
      <SequencingGel
        layout={creators({}, `${'N'.repeat(23)}👨‍👩‍👧tail`)}
        subject="Nervape 3x"
        active={null}
        onActive={() => {}}
      />
    );
    const value = [...container.querySelectorAll('[data-gene="Serial"] text')].at(-1)!;
    expect(value.textContent).not.toMatch(LONE_SURROGATE);
    expect(value.textContent).toBe(`${'N'.repeat(23)}…`);
  });

  it('takes two-line gene rows under a narrow map when a CJK reading does not fit', () => {
    stubChartWidth(608);
    // the reading "<raw> → <value>": Color's one byte and the arrow, 5 Latin glyphs (39 px), and
    // 26 CJK characters (422.5), 461.5 px, more than the third column's 608 − 88 − 91 − 20 = 409
    // (it was counted 241.8)
    render(
      <SequencingGel
        layout={creators({ value: '猫'.repeat(26) })}
        subject="Nervape 3x"
        active={null}
        onActive={() => {}}
      />
    );
    expect(screen.getByRole('list', { name: 'Genes' })).toHaveAttribute('data-rows', 'two-line');
  });

  it('makes the bytes column under a narrow map as wide as a CJK rule’s bound', () => {
    stubChartWidth(608);
    // the rule "颜色规则颜色规则", 8 CJK characters at the meta step (12 px), is 120 px, wider
    // than "nervapeColor" (86.4; it was counted 57.6)
    render(
      <SequencingGel
        layout={creators({ rule: '颜色规则颜色规则' })}
        subject="Nervape 3x"
        active={null}
        onActive={() => {}}
      />
    );
    const list = screen.getByRole('list', { name: 'Genes' });
    expect(list).toHaveAttribute('data-rows', 'one-line');
    const row = within(list).getAllByRole('listitem')[0].firstElementChild as HTMLElement;
    const bytesCol = Number(/^\d+px (\d+)px/.exec(row.style.gridTemplateColumns)![1]);
    expect(bytesCol).toBeGreaterThanOrEqual(bound(0, 8, 12));
  });

  /** Six lanes of CJK genes and band labels: at 400 px, lanes of (400 − 22) / 6 = 63 px. */
  const cjkLanes: AlleleLane[] = Array.from({ length: 6 }, (_, i) => ({
    gene: `颜色和图${i}`,
    bands: [
      { value: 'a', label: '红色的猫咪', count: 4, share: 4 / 6, isColour: false, colour: null },
      { value: 'b', label: 'b', count: 2, share: 2 / 6, isColour: false, colour: null },
    ],
    variants: 2,
    conserved: false,
    ordered: true,
  }));

  it('turns a CJK caption on end where its bound does not fit across its lane', () => {
    stubChartWidth(400);
    // "颜色和图0": 4 CJK characters and a digit at the tick size (12 px), 67.2 px, and 6 px
    // more than the 63 px lane (it was counted 36)
    const { container } = render(<AlleleGel lanes={cjkLanes} total={6} subject="deeds" />);
    const captions = [...container.querySelectorAll('[data-gene-caption]')];
    expect(captions).toHaveLength(6);
    for (const caption of captions) {
      const lane = caption.closest('[data-lane]')!.querySelector('rect')!;
      const laneWidth = Number(lane.getAttribute('width')) + 2;
      expect(laneWidth).toBeCloseTo(63, 5);
      if (!caption.hasAttribute('transform'))
        expect(bound(1, 4, 12) + 6, caption.textContent!).toBeLessThanOrEqual(laneWidth);
    }
  });

  it('cuts a CJK band label to its band', () => {
    stubChartWidth(400);
    // a 63 px lane writes its labels in a band of 63 − 2 × 7 = 49 px, 4 px clear each side: 41
    // px; "红色的猫咪" (5 CJK characters, 75 px at 12 px) was 5 code units, within the 5 glyphs
    // that fit, and drawn whole across the band's edges; 2 of them and the ellipsis are 37.2 px
    const { container } = render(<AlleleGel lanes={cjkLanes} total={6} subject="deeds" />);
    const labels = [...container.querySelectorAll('[data-lane] text[pointer-events="none"]')];
    const cut = labels.filter((l) => l.textContent !== 'b');
    expect(cut.map((l) => l.textContent)).toEqual(Array(6).fill('红色…'));
    expect(bound(1, 2, 12)).toBeLessThanOrEqual(63 - 2 * 7 - 8);
  });
});
