import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ByteScaleMap, HexMap, type ScaleSegment } from '@/components/ui/hex-map';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const segments: ScaleSegment[] = [
  {
    key: 'h',
    start: 0,
    end: 16,
    label: '16',
    short: '16',
    color: 'var(--c-ink-quiet)',
    quiet: true,
    title: 'header',
  },
  {
    key: 'n',
    start: 16,
    end: 31,
    label: 'name 15',
    short: '15',
    color: 'var(--c-cell-lock)',
    title: 'name',
    target: 'n',
  },
  {
    key: 'd',
    start: 31,
    end: 13890,
    label: 'description 13,859',
    short: '13,859',
    color: 'var(--c-data-framing)',
    title: 'description',
    target: 'd',
  },
  {
    key: 'm',
    start: 13890,
    end: 13890,
    label: '',
    color: 'var(--c-text-ghost)',
    title: 'mutantId',
    target: 'm',
  },
];

describe('ByteScaleMap (lane E, C5 / C6)', () => {
  it('draws each part as wide as its bytes with its name inside where it fits, and a ruler', () => {
    stubChartWidth(600);
    const { container } = render(
      <ByteScaleMap label="Cluster cell" total={13890} segments={segments} />
    );
    const parts = [...container.querySelectorAll<HTMLElement>('[data-segment]')];
    expect(parts.map((p) => p.dataset.segment)).toEqual(['h', 'n', 'd', 'm']);
    const widths = parts.map((p) => Number.parseFloat(p.style.width));
    // 3 gaps of 2 px, a 4 px sliver, the two short parts as wide as their byte counts
    expect(widths[3]).toBe(4);
    expect(widths[0]).toBeCloseTo(2 * 6.6 + 8);
    expect(widths.reduce((a, b) => a + b, 0) + 3 * 2).toBeCloseTo(600);
    expect(widths[2]).toBeGreaterThan(widths[0] * 20);
    // labels: the description has room for its name, the short parts for their byte counts
    expect(parts.map((p) => p.textContent)).toEqual(['16', '15', 'description 13,859', '']);
    const ruler = screen.getByRole('img', {
      name: 'Cluster cell offsets: 0, 16, 31, 13,890, 13,890',
    });
    expect(ruler.querySelectorAll('line')).toHaveLength(5);
    expect([...ruler.querySelectorAll('text')].map((t) => t.textContent)).toEqual([
      '0',
      '16',
      '31',
      '13,890',
    ]);
    expect(ruler.querySelector('text')).toHaveClass('text-meta');
  });

  it('makes parts with a target buttons that report hover and taps', () => {
    const onHover = vi.fn();
    const onSelect = vi.fn();
    render(
      <ByteScaleMap
        label="Cluster cell"
        total={13890}
        segments={segments}
        active="n"
        onHover={onHover}
        onSelect={onSelect}
      />
    );
    expect(screen.getAllByRole('button')).toHaveLength(3);
    const name = screen.getByRole('button', { name: 'name' });
    expect(name).toHaveAttribute('aria-pressed', 'true');
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'description' }));
    expect(onHover).toHaveBeenLastCalledWith('d');
    fireEvent.click(screen.getByRole('button', { name: 'mutantId' }));
    expect(onSelect).toHaveBeenCalledWith('m');
    fireEvent.mouseLeave(screen.getByRole('group', { name: 'Cluster cell byte map' }));
    expect(onHover).toHaveBeenLastCalledWith(null);
  });

  it('rejects parts that leave bytes out or overlap', () => {
    expect(() =>
      render(<ByteScaleMap label="Script" total={53} segments={segments.slice(0, 2)} />)
    ).toThrow('Script: byte map parts end at 31, not 53 bytes');
    expect(() =>
      render(
        <ByteScaleMap
          label="Script"
          total={31}
          segments={[segments[0], { ...segments[1], start: 15 }]}
        />
      )
    ).toThrow('Script: byte map part n [15, 31) breaks at 16');
  });

  it('keeps the byte-by-byte HexMap the default layout', () => {
    const { container } = render(
      <HexMap
        hex="0x0102"
        subject="witness"
        fields={[{ id: 'a', label: 'A', offset: 0, length: 1, description: 'A byte' }]}
      />
    );
    expect(container.querySelectorAll('[data-byte-offset]')).toHaveLength(2);
    expect(container.querySelector('[data-segment]')).toBeNull();
  });
});

describe('ByteScaleMap with many parts', () => {
  it('narrows the gaps to 1 px so a 72-gene ruler stays to scale in a 358 px phone column', () => {
    stubChartWidth(358);
    const many: ScaleSegment[] = [
      { key: 'name', start: 0, end: 40, label: 'name 40', color: 'var(--c-aqua)', title: 'name' },
      ...Array.from({ length: 71 }, (_, i) => ({
        key: `g${i}`,
        start: 40 + i,
        end: 41 + i,
        label: '',
        color: 'var(--c-object)',
        title: `g${i}`,
      })),
    ];
    const { container } = render(<ByteScaleMap label="DNA" total={111} segments={many} />);
    expect(screen.getByRole('group', { name: 'DNA byte map' })).toHaveStyle({ gap: '1px' });
    const widths = [...container.querySelectorAll<HTMLElement>('[data-segment]')].map((p) =>
      Number.parseFloat(p.style.width)
    );
    // 71 one-byte genes at the 3 px floor, the 40-byte gene with the rest — wider than 60 px
    expect(widths.slice(1).every((w) => w === 3)).toBe(true);
    expect(widths[0]).toBeGreaterThan(60);
    // a part with no label inside carries no padding, so a 3 px part is 3 px (the 390 px gate:
    // 4 px of padding a side made each stripe 8 px and pushed the ruler 324 px past the page)
    const parts = [...container.querySelectorAll<HTMLElement>('[data-segment]')];
    expect(parts[0]).toHaveStyle({ paddingLeft: '4px', paddingRight: '4px' });
    for (const p of parts.slice(1))
      expect(p).toHaveStyle({ paddingLeft: '0px', paddingRight: '0px' });
    for (const p of parts) expect(p).not.toHaveClass('px-1');
  });
});
