import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { AlignedBar, AlignedBarFootnote } from '@/components/ui/aligned-bar';
import { IciclePartition } from '@/components/ui/icicle-partition';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { TenureBar } from '@/components/ui/tenure-bar';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { SharedGraphicsDemo } from '@/__tests__/fixtures/shared-graphics-demo';

describe('AlignedBar (BA-01)', () => {
  it('starts every bar at one left baseline: fixed track first, value in a fixed column after', () => {
    const { container } = render(
      <>
        <AlignedBar
          label="Size, bytes"
          scale={4000}
          segments={[
            { key: 'overhead', value: 652, tone: 'overhead', label: 'overhead' },
            { key: 'user', value: 3_000, tone: 'data', label: 'user transactions' },
          ]}
        />
        <AlignedBar
          label="Size, bytes"
          scale={4000}
          segments={[{ key: 'overhead', value: 640, tone: 'overhead', label: 'block overhead' }]}
        />
      </>
    );
    const rows = Array.from(container.querySelectorAll('[data-aligned-bar]'));
    for (const row of rows) {
      const [track, value] = Array.from(row.children) as HTMLElement[];
      expect(track).toHaveAttribute('data-track');
      expect(track.style.width).toBe('120px');
      expect(track).toHaveClass('shrink-0');
      expect(value).toHaveClass('text-right', 'tabular-nums');
      // one width per table: the scale's own digits, so values right-align on one edge
      expect(value.style.width).toBe('5ch');
    }
    const segments = rows[0].querySelectorAll('[data-segment]');
    expect((segments[0] as HTMLElement).style.width).toBe('19.56px');
    expect((segments[1] as HTMLElement).style.width).toBe('90px');
    expect(rows[0]).toHaveTextContent('3,652');
    expect(
      screen.getByRole('img', { name: 'Size, bytes: 3,652, 652 overhead, 3,000 user transactions' })
    ).toBeInTheDocument();
  });

  it('refuses a value past its scale instead of clipping it', () => {
    expect(() =>
      render(
        <AlignedBar
          label="Cycles"
          scale={10}
          segments={[{ key: 'c', value: 11, tone: 'data', label: 'cycles' }]}
        />
      )
    ).toThrow('Cycles: 11 exceeds the scale 10');
  });

  it('puts the consensus cap in a footnote, not on the bar', () => {
    render(<AlignedBarFootnote scale="0–4,000 B" cap="the consensus limit is 597,000 B" />);
    expect(screen.getByText(/0–4,000 B/)).toHaveTextContent(
      'Scale 0–4,000 B · the consensus limit is 597,000 B'
    );
  });
});

describe('IciclePartition (DAO-7)', () => {
  const root = {
    key: 'total',
    label: 'Secondary issuance since genesis',
    value: BigInt(100),
    tone: 'ckb' as const,
    children: [
      {
        key: 'miners',
        label: 'to miners',
        value: BigInt(10),
        tone: 'ckb' as const,
        placeholder: 'paid in cellbases',
      },
      {
        key: 'comp',
        label: 'compensation for depositors',
        value: BigInt(20),
        tone: 'dao' as const,
        children: [
          { key: 'paid', label: 'paid out', value: BigInt(5), tone: 'dao' as const },
          {
            key: 'owed',
            label: 'still owed',
            value: BigInt(15),
            tone: 'dao' as const,
            opacity: 0.4,
          },
        ],
      },
      {
        key: 'burnt',
        label: 'burnt',
        value: BigInt(70),
        tone: 'other' as const,
        placeholder: 'No cell can ever claim it.',
      },
    ],
  };
  it('aligns each split under its parent on a fixed viewBox', () => {
    const { container } = render(
      <IciclePartition root={root} label="Where secondary issuance goes" formatValue={String} />
    );
    const svg = container.querySelector('svg')!;
    expect(svg.getAttribute('viewBox')).toBe('0 0 900 240');
    expect(svg).toHaveAttribute('width', '100%');
    const box = (key: string) => container.querySelector(`[data-node="${key}"]`)!;
    expect(box('total').getAttribute('width')).toBe('900');
    expect(box('miners').getAttribute('width')).toBe('90');
    expect(box('comp').getAttribute('x')).toBe('90');
    // the compensation's children sit exactly under it
    expect(box('paid').getAttribute('x')).toBe('90');
    expect(
      Number(box('paid').getAttribute('width')) + Number(box('owed').getAttribute('width'))
    ).toBe(180);
    expect(box('comp').getAttribute('fill')).toBe(CLASS_COLORS.dao);
    expect(container.querySelector('[data-placeholder="burnt"]')).toHaveTextContent(
      'No cell can ever claim it.'
    );
    expect(screen.getByRole('img', { name: 'Where secondary issuance goes' })).toBeInTheDocument();
  });
  it('refuses children that do not partition their parent', () => {
    const bad = { ...root, children: [{ ...root.children[0], value: BigInt(11) }] };
    expect(() => render(<IciclePartition root={bad} label="x" formatValue={String} />)).toThrow(
      'Secondary issuance since genesis: parts sum to 11, not the total 100'
    );
  });
});

// ---- F2 group dao: additive IciclePartition options (DAO-7 on the DAO page) ----
describe('IciclePartition label placement and pixel width (F2 dao)', () => {
  const node = (over: Record<string, unknown>) => ({
    key: 'total',
    label: 'Secondary issuance since genesis',
    value: BigInt(100),
    tone: 'ckb' as const,
    ...over,
  });
  const tree = node({
    align: 'inside',
    note: '1.344 billion CKB a year',
    color: '#4b5266',
    ink: 'bright',
    children: [
      { key: 'm', label: 'to miners', value: BigInt(10), tone: 'ckb' as const, note: '10.00%' },
      {
        key: 'c',
        label: 'compensation',
        value: BigInt(20),
        tone: 'dao' as const,
        align: 'end' as const,
        note: '20.00% · via AR',
      },
      {
        key: 'b',
        label: 'burnt',
        value: BigInt(70),
        tone: 'other' as const,
        align: 'inside' as const,
        ink: 'bright' as const,
      },
    ],
  });
  it('draws at a measured pixel width, with a fill override and three label placements', () => {
    const { container } = render(
      <IciclePartition root={tree} label="issuance" formatValue={String} width={1200} />
    );
    const svg = container.querySelector('svg')!;
    // The root's words sit inside it, so row 1 follows 16 px below it, not a label band.
    expect(svg.getAttribute('viewBox')).toBe('0 0 1200 116');
    const box = (key: string) => container.querySelector(`[data-node="${key}"]`)!;
    expect(box('total').getAttribute('fill')).toBe('#4b5266');
    expect(box('c').getAttribute('x')).toBe('120');
    // inside: one line in the box; end: right-aligned under the box; default: left under it
    const label = (key: string) => container.querySelector(`[data-label="${key}"]`)!;
    expect(label('total').getAttribute('y')).toBe('19');
    expect(label('total')).toHaveTextContent(
      'Secondary issuance since genesis 100 · 1.344 billion CKB a year'
    );
    expect(label('total')).toHaveClass('fill-text-bright');
    expect(label('c').getAttribute('text-anchor')).toBe('end');
    expect(label('c').getAttribute('x')).toBe('350');
    expect(container.querySelector('[data-note="c"]')).toHaveTextContent('20.00% · via AR');
    expect(label('m').getAttribute('text-anchor')).toBe('start');
    expect(label('b').getAttribute('y')).toBe(String(44 + 19));
  });
  it('can leave every word to an HTML legend at narrow widths', () => {
    const { container } = render(
      <IciclePartition root={tree} label="issuance" formatValue={String} width={358} hideLabels />
    );
    expect(container.querySelectorAll('text')).toHaveLength(0);
    // rows sit one box apart once no labels need the room between them
    expect(container.querySelector('svg')!.getAttribute('viewBox')).toBe('0 0 358 64');
    expect(container.querySelector('[data-node="b"]')!.getAttribute('y')).toBe('36');
  });
});

// ---- hierarchy v3 (DAO page, R12): additive IciclePartition options ----
describe('IciclePartition rows and words inside the boxes (hierarchy v3, R9, R12)', () => {
  const tree = {
    key: 'total',
    label: 'Secondary issuance since genesis',
    value: BigInt(100),
    tone: 'ckb' as const,
    color: '#4b5266',
    ink: 'bright' as const,
    boxLabel: { name: 'secondary issuance since genesis', share: '100', unit: 'CKB' },
    children: [
      {
        key: 'm',
        label: 'to miners',
        value: BigInt(10),
        tone: 'ckb' as const,
        color: '#a0a8be',
        ink: 'dark' as const,
        boxLabel: { name: 'miners', share: '10.00%' },
      },
      {
        key: 'c',
        label: 'compensation',
        value: BigInt(20),
        tone: 'dao' as const,
        ink: 'dark' as const,
        boxLabel: { name: 'depositors', share: '20.00%' },
        children: [
          {
            key: 'p',
            label: 'paid out',
            value: BigInt(10),
            tone: 'dao' as const,
            ink: 'dark' as const,
            boxLabel: { name: 'paid out', share: '50.0%' },
          },
          {
            key: 'o',
            label: 'still owed',
            value: BigInt(10),
            tone: 'dao' as const,
            ink: 'bright' as const,
            boxLabel: { name: 'still owed', share: '50.0%' },
          },
        ],
      },
      {
        key: 'b',
        label: 'burnt',
        value: BigInt(70),
        tone: 'other' as const,
        ink: 'bright' as const,
        boxLabel: { name: 'burnt', share: '70.00%', note: 'no cell can ever claim it' },
      },
    ],
  };
  const rows = { heights: [48, 48, 32], gap: 3 };

  it('stacks rows of the given heights a gap apart and sets sibling boxes a gap apart', () => {
    const { container } = render(
      <IciclePartition root={tree} label="issuance" formatValue={String} width={1200} rows={rows} />
    );
    expect(container.querySelector('svg')!.getAttribute('viewBox')).toBe('0 0 1200 134');
    const box = (key: string) => container.querySelector(`[data-node="${key}"]`)!;
    expect(['total', 'm', 'p'].map((k) => box(k).getAttribute('y'))).toEqual(['0', '51', '102']);
    expect(['total', 'm', 'p'].map((k) => box(k).getAttribute('height'))).toEqual([
      '48',
      '48',
      '32',
    ]);
    // Siblings share the row less the gaps: (1200 − 2 × 3) split 10 : 20 : 70.
    expect(box('m').getAttribute('x')).toBe('0');
    expect(box('m').getAttribute('width')).toBe('119.4');
    expect(box('c').getAttribute('x')).toBe('122.4');
    expect(box('b').getAttribute('x')).toBe('364.2');
    // compensation's two parts sit exactly under it, a gap apart
    expect(box('p').getAttribute('x')).toBe('122.4');
    expect(box('o').getAttribute('x')).toBe('243.3');
  });

  it('writes the name and share inside each box, falling back to the share alone where the words do not fit', () => {
    const { container } = render(
      <IciclePartition root={tree} label="issuance" formatValue={String} width={1200} rows={rows} />
    );
    const label = (key: string) => container.querySelector(`[data-label="${key}"]`);
    expect(label('total')).toHaveTextContent('secondary issuance since genesis 100 CKB');
    expect(label('total')!.getAttribute('y')).toBe('29');
    // 119 px holds "10.00%" but not "miners 10.00%" at 13 px
    expect(label('m')!.textContent).toBe('10.00%');
    expect(label('c')!.textContent).toBe('depositors 20.00%');
    expect(label('b')!.textContent).toBe('burnt 70.00% · no cell can ever claim it');
    expect(label('b')!.querySelector('[data-part="note"]')).toHaveClass('fill-text-dim');
    // the 32 px row reads at 11 px: "paid out 50.0%" fits 118 px, "still owed 50.0%" does not
    expect(label('p')!.textContent).toBe('paid out 50.0%');
    expect(label('p')).toHaveAttribute('font-size', '11');
    expect(label('o')!.textContent).toBe('50.0%');
    // ink: the page colour on light boxes, the bright ink on dark ones
    expect(label('m')).toHaveClass('fill-base-bg');
    expect(label('b')).toHaveClass('fill-text-bright');
    // no words under the boxes (R12)
    expect(container.querySelectorAll('[data-note]')).toHaveLength(0);
  });

  it('writes nothing in a box too narrow for its share', () => {
    const { container } = render(
      <IciclePartition root={tree} label="issuance" formatValue={String} width={358} rows={rows} />
    );
    expect(container.querySelector('[data-label="m"]')).toBeNull();
    expect(container.querySelector('[data-label="b"]')!.textContent).toBe('burnt 70.00%');
  });
});

describe('ConcentrationBar (DAO-12)', () => {
  it('splits a total into ranked groups of one class colour with exact shares', () => {
    const { container } = render(
      <ConcentrationBar
        label="Share of deposits"
        assetClass="dao"
        total={BigInt(1000)}
        groups={[
          { label: 'largest', value: BigInt(83) },
          { label: 'next 9', value: BigInt(179) },
          { label: 'next 90', value: BigInt(311) },
          { label: 'the other 16,541', value: BigInt(427) },
        ]}
      />
    );
    const segments = Array.from(container.querySelectorAll('[data-segment]')) as HTMLElement[];
    expect(segments.map((s) => s.style.width)).toEqual(['8.3%', '17.9%', '31.1%', '42.7%']);
    expect(segments.map((s) => s.style.opacity)).toEqual(['1', '0.7', '0.45', '0.22']);
    for (const s of segments) expect(s.style.backgroundColor).toBe('rgb(242, 197, 92)');
    expect(screen.getByText('next 9').parentElement).toHaveTextContent('next 9 17.90%');
  });
  it('refuses groups that do not add up to the total', () => {
    expect(() =>
      render(
        <ConcentrationBar
          label="x"
          assetClass="tokens"
          total={BigInt(10)}
          groups={[{ label: 'a', value: BigInt(9) }]}
        />
      )
    ).toThrow('x: parts sum to 9, not the total 10');
  });
});

// ---- hierarchy v3 (DAO page, R15): additive ConcentrationBar option ----
describe('ConcentrationBar with the group names inside its segments (hierarchy v3, R15)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const groups = [
    { label: 'largest', value: BigInt(83) },
    { label: 'next 9', value: BigInt(179) },
    { label: 'next 90', value: BigInt(311) },
    { label: 'the other 16,541', value: BigInt(427) },
  ];
  const bar = (width: number) => {
    stubChartWidth(width);
    return render(
      <ConcentrationBar
        label="Share of deposits"
        assetClass="dao"
        total={BigInt(1000)}
        groups={groups}
        labelsInside
      />
    );
  };

  it('draws a 28 px bar with each name and share inside its segment, and no legend row', () => {
    const { container } = bar(1408);
    const img = screen.getByRole('img', { name: /^Share of deposits/ });
    expect(img).toHaveClass('h-[28px]');
    const segments = Array.from(container.querySelectorAll('[data-segment]')) as HTMLElement[];
    expect(segments.map((s) => s.textContent)).toEqual([
      'largest 8.30%',
      'next 9 · 17.90%',
      'next 90 · 31.10%',
      'the other 16,541 · 42.70%',
    ]);
    // The fade is the background's, so the words keep full ink: dark on the strong segments.
    expect(segments.map((s) => s.style.opacity)).toEqual(['', '', '', '']);
    expect(segments[0]).toHaveClass('text-base-bg');
    expect(segments[3]).toHaveClass('text-text-bright');
    expect(container.querySelector('[data-role="concentration-legend"]')).toBeNull();
  });

  it('keeps the legend row when a segment is too narrow for its words', () => {
    const { container } = bar(358);
    const segments = Array.from(container.querySelectorAll('[data-segment]')) as HTMLElement[];
    // 8.3 % of 358 px holds neither "largest 8.30%" nor "8.30%"
    expect(segments[0].textContent).toBe('');
    expect(container.querySelector('[data-role="concentration-legend"]')).toHaveTextContent(
      'largest 8.30%'
    );
  });

  it('is the original thin bar with a legend row when not asked', () => {
    stubChartWidth(1408);
    const { container } = render(
      <ConcentrationBar label="x" assetClass="dao" total={BigInt(1000)} groups={groups} />
    );
    expect(screen.getByRole('img', { name: /^x/ })).toHaveClass('h-3');
    expect(container.querySelector('[data-segment="largest"]')!.textContent).toBe('');
    expect(screen.getByText('next 9').parentElement).toHaveTextContent('next 9 17.90%');
  });
});

describe('TenureBar (ID-C7)', () => {
  const day = 86_400_000;
  const reg = Date.parse('2024-02-20T00:00:00Z');
  it('draws registration and renewals, elapsed dim and remaining bright, with a now marker', () => {
    const { container } = render(
      <TenureBar
        name="valar.bit"
        periods={[
          { label: 'paid at registration', start: reg, end: reg + 365 * day },
          { label: 'renewed 2025-02-04', start: reg + 365 * day, end: reg + 730 * day },
          { label: 'renewed 2025-02-18', start: reg + 730 * day, end: reg + 1095 * day },
        ]}
        now={reg + 979 * day}
      />
    );
    const svg = container.querySelector('svg')!;
    expect(svg.getAttribute('viewBox')).toBe('0 0 900 92');
    expect(container.querySelectorAll('[data-elapsed]').length).toBe(3);
    expect(container.querySelectorAll('[data-remaining]').length).toBe(1);
    // Lane P: the now mark is dated; the days left are its hover.
    const nowMark = container.querySelector('[data-role="tenure-now"]')!;
    expect(nowMark.lastChild!.textContent).toMatch(/^now · \d{4}-\d\d-\d\d$/);
    expect(nowMark.querySelector('title')).toHaveTextContent('116 days left');
    expect(screen.getByRole('heading', { name: 'Held until 2027-02-19' })).toBeInTheDocument();
    expect(container.querySelector('[data-remaining]')).toHaveAttribute(
      'fill',
      CLASS_COLORS.identities
    );
  });
  it('draws a dashed grace period and an expired state in neutral grey', () => {
    const { container, rerender } = render(
      <TenureBar
        name="apt.cell"
        periods={[{ label: 'registered, one year', start: reg, end: reg + 365 * day }]}
        graceEnd={reg + 395 * day}
        now={reg + 100 * day}
      />
    );
    const grace = container.querySelector('[data-grace]')!;
    expect(grace).toHaveAttribute('stroke-dasharray', '3 3');
    expect(screen.getByText('grace · 30 days')).toBeInTheDocument();
    rerender(
      <TenureBar
        name="888899.bit"
        periods={[{ label: 'paid at registration', start: reg, end: reg + 365 * day }]}
        now={reg + 549 * day}
      />
    );
    expect(screen.getByRole('heading', { name: 'Expired on 2025-02-19' })).toBeInTheDocument();
    // Lane P: the now mark is dated; how long ago the term ended is its hover.
    const nowMark = container.querySelector('[data-role="tenure-now"]')!;
    expect(nowMark.lastChild!.textContent).toMatch(/^now · \d{4}-\d\d-\d\d$/);
    expect(nowMark.querySelector('title')).toHaveTextContent('expired 184 days ago');
    expect(container.querySelector('[data-elapsed]')).not.toHaveAttribute(
      'fill',
      CLASS_COLORS.identities
    );
  });
  // F2 identities (round 2): the board's caption beside the title (IdentityBit "3 years paid").
  it('sets an optional caption beside its title', () => {
    render(
      <TenureBar
        name="valar.bit"
        periods={[{ label: 'paid at registration', start: reg, end: reg + 365 * day }]}
        now={reg + 100 * day}
        note="1 year paid at registration · not renewed"
      />
    );
    const heading = screen.getByRole('heading', { name: 'Held until 2025-02-19' });
    expect(heading.parentElement).toHaveTextContent('1 year paid at registration · not renewed');
  });
});

// F2 identities (round 2, visual pass at 390): the bar is drawn in its own measured width so its
// 11 px labels stay 11 px; edge labels stay inside; an expired term shows its lapsed time.
describe('TenureBar at any width (ID-C7, visual pass)', () => {
  const day = 86_400_000;
  const reg = Date.parse('2024-02-20T00:00:00Z');
  const three = [
    { label: 'paid at registration', start: reg, end: reg + 365 * day },
    { label: 'renewed 2025-02-04', start: reg + 365 * day, end: reg + 730 * day },
    { label: 'renewed 2025-02-18', start: reg + 730 * day, end: reg + 1095 * day },
  ];
  afterEach(() => vi.unstubAllGlobals());
  it('uses its measured width as the drawing width and shortens labels that do not fit', () => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(private cb: (e: { contentRect: { width: number } }[]) => void) {}
        observe() {
          this.cb([{ contentRect: { width: 358 } }]);
        }
        disconnect() {}
      }
    );
    const { container } = render(
      <TenureBar name="valar.bit" periods={three} now={reg + 979 * day} />
    );
    expect(container.querySelector('svg')!.getAttribute('viewBox')).toBe('0 0 358 92');
    expect(screen.queryByText('renewed 2025-02-04')).not.toBeInTheDocument();
    expect(screen.getAllByText('renewed').length).toBe(2);
    // Lane P: the now mark is dated; the days left are its hover.
    const nowMark = container.querySelector('[data-role="tenure-now"]')!;
    expect(nowMark.lastChild!.textContent).toMatch(/^now · \d{4}-\d\d-\d\d$/);
    expect(nowMark.querySelector('title')).toHaveTextContent('116 days left');
  });
  it('keeps the now label inside at the right edge and draws the lapsed time dashed', () => {
    const { container } = render(
      <TenureBar
        name="888899.bit"
        periods={[{ label: 'from minting', start: reg, end: reg + 365 * day }]}
        now={reg + 549 * day}
      />
    );
    expect(container.querySelector('[data-role="tenure-now"]')).toHaveAttribute(
      'text-anchor',
      'end'
    );
    const lapsed = container.querySelector('[data-lapsed]')!;
    expect(lapsed).toHaveAttribute('stroke-dasharray', '3 3');
    expect(screen.getByText('lapsed · not renewed')).toBeInTheDocument();
  });
  it('writes day counts with thousands separators, as the rail does', () => {
    const { container } = render(
      <TenureBar
        name="alice.bit"
        periods={[{ label: 'paid at registration', start: reg, end: reg + 4000 * day }]}
        now={reg + 100 * day}
      />
    );
    // Lane P: the days left are the now mark's hover.
    expect(container.querySelector('[data-role="tenure-now"] title')).toHaveTextContent(
      /^3,900 days left$/
    );
  });
});

describe('shared graphics demo (fixture page, no route)', () => {
  it.each([1440, 390])(
    'fits a %i px column: every graphic scales by viewBox or percentages',
    (width) => {
      const { container } = render(<SharedGraphicsDemo width={width} />);
      const page = container.firstElementChild as HTMLElement;
      expect(page.style.width).toBe(`${width}px`);
      expect(page).toHaveClass('min-w-0', 'overflow-hidden');
      for (const svg of Array.from(container.querySelectorAll('svg[viewBox]')))
        if (svg.getAttribute('width') !== null && !/^\d+$/.test(svg.getAttribute('width')!))
          expect(svg.getAttribute('width')).toBe('100%');
      // The only fixed-width parts are the 120 px bar tracks and their value column: < 390 px.
      for (const row of Array.from(container.querySelectorAll('[data-aligned-bar]')))
        expect(row).toHaveClass('min-w-0');
      expect(within(page).getAllByRole('img').length).toBeGreaterThan(4);
    }
  );
});
