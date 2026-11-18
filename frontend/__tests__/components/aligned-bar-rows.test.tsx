// Table-rows round, lane R2: the lifted ScaleHeader and AlignedBar's height, minimum fill,
// display slot and class tones. Every default is the drawing the existing bars already had.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ALIGNED_BAR_HEIGHT, AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { ScaleHeader as BlockVisualScaleHeader } from '@/components/blocks/block-visual';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';

const ticksOf = (root: ParentNode) => root.querySelector('[data-scale-ticks]') as HTMLElement;

describe('ScaleHeader, one export in aligned-bar.tsx (table-rows R2)', () => {
  it('is the same component block-visual has always exported', () => {
    expect(BlockVisualScaleHeader).toBe(ScaleHeader);
  });

  it('draws the name over 0 … scale, as wide as the track, 3 px apart by default (Blocks)', () => {
    const { container } = render(<ScaleHeader label="Size, bytes" scale={4000} />);
    const head = container.firstElementChild as HTMLElement;
    expect(head).toHaveClass('flex', 'flex-col');
    expect(head.style.gap).toBe('3px');
    expect(head).toHaveTextContent(/^Size, bytes04,000$/);
    // T5 (coordinator ruling, lane M): the name is a column head, 11 px like every head.
    expect(head.firstElementChild).toHaveTextContent(/^Size, bytes$/);
    expect(head.firstElementChild).toHaveClass('text-meta');
    const ticks = ticksOf(head);
    expect(ticks).toHaveAttribute('aria-hidden', 'true');
    expect(ticks).toHaveClass('text-meta', 'flex', 'justify-between');
    expect(ticks.style.width).toBe('120px');
    expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', '4,000']);
  });

  it('takes the track width and the gap a table has always drawn (transactions: 80 px, 2 px)', () => {
    const { container } = render(
      <ScaleHeader label="Cycles" scale={2_000_000} trackWidth={80} tickGap={2} />
    );
    const head = container.firstElementChild as HTMLElement;
    expect(head.style.gap).toBe('2px');
    expect(ticksOf(head).style.width).toBe('80px');
    expect(head).toHaveTextContent(/^Cycles02,000,000$/);
  });

  it('formats its ticks and can mark the middle of the scale ("0 · 25B · 50B")', () => {
    render(
      <ScaleHeader
        label="CKB in its cells"
        scale={50_000_000_000}
        midpoint
        format={(n) => `${n / 1e9}B`}
      />
    );
    const ticks = ticksOf(document.body);
    expect(Array.from(ticks.children).map((t) => t.textContent)).toEqual(['0', '25B', '50B']);
  });

  // Readability 2026-11-18, lane B5: in a column that holds a bar and its figure, the head's name
  // stood over the bar and the figure had none. Given the figure's width, the name ends where the
  // figure ends (right of the track and the bar's 10 px gap); the ticks stay over the track.
  it('stands its name over the figure when it is told the figure’s width (B5)', () => {
    const { container } = render(
      <ScaleHeader label="Occupied" scale={1_000_000} trackWidth={80} figureWidth={51} />
    );
    const bar = render(
      <AlignedBar
        label="Occupied"
        scale={1_000_000}
        trackWidth={80}
        segments={[{ key: 'o', label: 'occupied', value: 42_000, tone: 'data' }]}
        display="4.20%"
        displayWidth={51}
      />
    ).container;
    const head = container.firstElementChild as HTMLElement;
    const name = head.firstElementChild as HTMLElement;
    expect(name).toHaveTextContent(/^Occupied$/);
    expect(name).toHaveClass('text-meta', 'flex', 'justify-end', 'whitespace-nowrap');
    // the bar's own geometry: its track, its 10 px gap, its figure
    const row = bar.querySelector('[data-aligned-bar]') as HTMLElement;
    expect(row).toHaveClass('gap-2.5');
    const track = row.querySelector<HTMLElement>('[data-track]')!;
    const figure = row.lastElementChild as HTMLElement;
    expect(name.style.width).toBe(
      `${Number.parseFloat(track.style.width) + 10 + Number.parseFloat(figure.style.width)}px`
    );
    expect(ticksOf(head).style.width).toBe(track.style.width);
  });

  it('refuses a figure width it cannot draw', () => {
    expect(() => ScaleHeader({ label: 'x', scale: 10, figureWidth: 0 })).toThrow(
      'x: invalid figure width 0'
    );
  });

  it('refuses a scale or a gap it cannot draw', () => {
    expect(() => ScaleHeader({ label: 'x', scale: 0 })).toThrow('x: invalid column scale 0');
    expect(() => ScaleHeader({ label: 'x', scale: 2.5 })).toThrow('x: invalid column scale 2.5');
    expect(() => ScaleHeader({ label: 'x', scale: 10, tickGap: -1 })).toThrow(
      'x: invalid tick gap -1'
    );
  });
});

describe('AlignedBar height and minimum fill (table-rows R2, T4)', () => {
  const bar = (props: Partial<Parameters<typeof AlignedBar>[0]> = {}, value = 4) =>
    render(
      <AlignedBar
        label="CKB in its cells"
        scale={4000}
        segments={[{ key: 'ckb', label: 'CKB', value, tone: 'data' }]}
        {...props}
      />
    ).container;
  const trackOf = (c: HTMLElement) => c.querySelector('[data-track]') as HTMLElement;
  const fillOf = (c: HTMLElement) => c.querySelector('[data-segment="ckb"]') as HTMLElement;

  it('is 8 px tall by default and takes another height', () => {
    expect(ALIGNED_BAR_HEIGHT).toBe(8);
    expect(trackOf(bar()).style.height).toBe('8px');
    expect(trackOf(bar({ height: 6 })).style.height).toBe('6px');
  });

  it('draws a value at its exact width by default, even under a pixel (existing bars unchanged)', () => {
    // 4 of 4,000 on a 120 px track: 0.12 px.
    expect(fillOf(bar()).style.width).toBe('0.12px');
  });

  it('with minFill draws any nonzero value at least that wide, and zero as nothing', () => {
    expect(fillOf(bar({ minFill: 2 })).style.width).toBe('2px');
    expect(fillOf(bar({ minFill: 2 }, 0)).style.width).toBe('0px');
    expect(fillOf(bar({ minFill: 2 }, 2000)).style.width).toBe('60px');
  });

  it('refuses a height or a minimum it cannot draw', () => {
    expect(() => bar({ height: 0 })).toThrow('CKB in its cells: invalid bar height 0');
    expect(() => bar({ minFill: -1 })).toThrow('CKB in its cells: invalid minimum fill -1');
    expect(() => bar({ minFill: 121 })).toThrow('CKB in its cells: invalid minimum fill 121');
  });
});

describe('AlignedBar display and class tones (table-rows R2)', () => {
  it('prints a display in place of the integer total, right-aligned in displayWidth px', () => {
    const { container } = render(
      <AlignedBar
        label="CKB in its cells"
        scale={50_000_000_000}
        segments={[{ key: 'ckb', label: 'CKB', value: 46_743_011_404, tone: 'data' }]}
        display={
          <>
            46,743,011,404<span className="text-meta">.24</span>
          </>
        }
        displayWidth={118}
      />
    );
    const [track, value] = Array.from(
      container.querySelector('[data-aligned-bar]')!.children
    ) as HTMLElement[];
    expect(value).toHaveTextContent(/^46,743,011,404\.24$/);
    expect(value).toHaveClass('text-right', 'tabular-nums');
    expect(value.style.width).toBe('118px');
    // The printed amount is the value; the track only pictures it.
    expect(track).toHaveAttribute('aria-hidden', 'true');
    expect(screen.queryByRole('img')).toBeNull();
  });

  it('keeps the exact integer total, its ch column and the described track without a display', () => {
    const { container } = render(
      <AlignedBar
        label="Peers"
        scale={400}
        segments={[{ key: 'n', label: 'peers', value: 312, tone: 'data' }]}
      />
    );
    const value = container.querySelector('[data-aligned-bar]')!.children[1] as HTMLElement;
    expect(value).toHaveTextContent('312');
    expect(value.style.width).toBe('3ch');
    expect(screen.getByRole('img', { name: 'Peers: 312' })).toBeInTheDocument();
  });

  it('colours a segment by asset class (tokens, objects, identities, dao), data jade', () => {
    const { container } = render(
      <AlignedBar
        label="CKB held"
        scale={100}
        segments={[
          { key: 'tokens', label: 'tokens', value: 10, tone: 'tokens' },
          { key: 'objects', label: 'objects', value: 10, tone: 'objects' },
          { key: 'identities', label: 'identities', value: 10, tone: 'identities' },
          { key: 'dao', label: 'dao', value: 10, tone: 'dao' },
          { key: 'data', label: 'data', value: 10, tone: 'data' },
        ]}
      />
    );
    const colour = (key: string) =>
      (container.querySelector(`[data-segment="${key}"]`) as HTMLElement).style.backgroundColor;
    // the class colours are theme references, kept as written by jsdom (a var() is not parsed)
    expect(colour('tokens')).toBe(CLASS_COLORS.tokens);
    expect(colour('objects')).toBe(CLASS_COLORS.objects);
    expect(colour('identities')).toBe(CLASS_COLORS.identities);
    expect(colour('dao')).toBe(CLASS_COLORS.dao);
    expect(colour('data')).toBe(DATA_COLOR);
  });
});
