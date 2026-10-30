import { describe, expect, it } from 'vitest';
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
    expect(screen.getByText('now · 116 days left')).toBeInTheDocument();
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
    expect(screen.getByText('expired 184 days ago')).toBeInTheDocument();
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
