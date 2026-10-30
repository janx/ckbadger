import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { SequencingGel } from '@/components/ui/sequencing-gel';
import { AlleleGel } from '@/components/ui/allele-gel';
import { alleleLanes, geneOrder, genotypePoints, sequencingGenes } from '@/lib/dob-genes';
import { BITCOIN_COLORS, GENE_COLORS } from '@/lib/design-tokens';
import { nervape3xGenetics, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';

const layout = sequencingGenes(nervape3xGenetics, 'Nervape 3x');
const lanes = alleleLanes(
  nervape3xInsights.genetics,
  geneOrder(nervape3xInsights.currentPattern),
  'Nervape 3x'
);

describe('SequencingGel (Object board, OB-7)', () => {
  it('draws every hex digit in its own lane, top to bottom, with a byte ruler', () => {
    const { container } = render(
      <SequencingGel layout={layout} subject="Nervape 3x" active={null} onActive={() => {}} />
    );
    const bands = container.querySelectorAll('[data-nibble]');
    expect(bands).toHaveLength(106);
    // a9 → nibble 0 in lane a, nibble 1 in lane 9; lanes are 15 units wide from x = 64.
    expect(Number(bands[0].getAttribute('x'))).toBeCloseTo(64 + 10 * 15 + 2.5, 5);
    expect(Number(bands[1].getAttribute('x'))).toBeCloseTo(64 + 9 * 15 + 2.5, 5);
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
