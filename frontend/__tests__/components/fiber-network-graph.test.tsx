import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '../utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  FiberPartiesNote,
  FiberNetworkPicture,
  FiberNetworkReading,
  useFiberGraphPin,
} from '@/components/fiber/fiber-network-graph';
import network from '../fixtures/fiber-network.json';
import mainnet from '../fixtures/fiber-network-mainnet.json';
import type { FiberNetwork } from '@/lib/api';
import { fullText } from '@/__tests__/utils/full-text';
import {
  allTextLines,
  expectApart,
  expectInside,
  textLines,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';
import { MEASURE_CHARS } from '@/lib/type-metrics';

const node = (tail: string) => screen.getByRole('button', { name: new RegExp(`^Party …${tail},`) });
const panel = () => screen.getByRole('complementary', { name: 'Party or pair' });
/** The chosen party's or pair's card, under the legend in the same column. */
const card = () => panel().querySelector('[data-role="party-or-pair"]') as HTMLElement;
const readout = () => screen.getByRole('status', { name: 'Party or pair readout' });
const IDLE = 'a party’s channels and partners, or a pair’s channels';

/** The page's wiring (hierarchy v3): the picture in the hero section, its reading under it. */
function Graph({ network: net }: { network: FiberNetwork }) {
  const pin = useFiberGraphPin();
  return (
    <>
      <section aria-label="hero">
        <FiberNetworkPicture network={net} pin={pin} />
      </section>
      <div data-testid="under">
        <FiberNetworkReading network={net} pin={pin} />
      </div>
    </>
  );
}

describe('FiberNetworkGraph', () => {
  // The graph is laid out for its measured column (R4); 880 px is its desktop column at 1280.
  beforeEach(() => stubChartWidth(880));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws the network with its name inside the plot and the board’s legend at its top right (FF-F1, R13)', () => {
    const { container } = render(<Graph network={mainnet as FiberNetwork} />);
    // hierarchy v3: no separate h2 for the hero's picture; its name is written inside the plot
    expect(screen.queryByRole('heading', { name: 'The network, as the chain sees it' })).toBeNull();
    const svg = container.querySelector('svg[data-fiber-graph]')!;
    const name = svg.querySelector('[data-role="chart-name"]')!;
    expect(name).toHaveTextContent('the network, as the chain sees it');
    // re-pinned (readability R4): the label role is 12 px (was 11)
    expect(name).toHaveAttribute('font-size', '12');
    expect(Number(name.getAttribute('x'))).toBeLessThanOrEqual(8);
    // the legend heads the column at the plot's right, before any party or pair is chosen
    const key = within(panel()).getByRole('list', { name: 'Graph key' });
    expect(
      within(key)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
      // Re-pinned on purpose (lane P4, the plan's P4 sentence: the Fiber graph's numbers and sizes
      // come up from tier 2 as a legend): the key names what a circle's size and number are and
      // what each line's width is; it named the lines' colours and the ring only, the rest was
      // tier 2's prose ("How to read the graph")
    ).toEqual([
      'party · size and number: its channels',
      'has an open channel',
      'pair · grey width: channels over time',
      'green width: CKB locked now',
      'force-closed',
    ]);
    expect(key).toHaveClass('text-meta', 'text-text-dim');
    expect(svg.querySelectorAll('[data-node]')).toHaveLength(25);
    expect(svg.querySelectorAll('[data-edge]')).toHaveLength(35);
    expect(within(svg as HTMLElement).getByText('kdeakp')).toBeInTheDocument();
    // a hub shows its channel count inside the circle
    expect(within(node('kdeakp')).getByText('165')).toBeInTheDocument();
    // dashed offset lines for pairs with a force-closed channel
    expect(svg.querySelectorAll('[data-forced]').length).toBeGreaterThan(0);
    expect(svg.querySelectorAll('[data-forced]')[0]).toHaveAttribute('stroke-dasharray', '3 3');
    // the small groups sit in the right strip with the board's note
    const note = screen.getByText('not linked to the rest');
    // anchored to the canvas edge (visual pass, 1440)
    expect(note).toHaveAttribute('text-anchor', 'end');
    expect(Number(note.getAttribute('x'))).toBeLessThanOrEqual(880);
    // tier 1 carries no developer sentence about represented histories (FF-F6)
    expect(screen.queryByText(/channel histories represented/)).toBeNull();
  });

  // The plan's P4 sentence ("keys that a tier-1 figure needs come up from tier 2 as a legend, not
  // as prose": the Fiber graph's numbers and sizes). The key draws each mark as the figure draws
  // it: a party's circle with its count inside, its ring, and a pair's grey, green and dashed
  // lines, the grey at the strength the edges are drawn at (review fiber-channels: the grey
  // "channels over time" edges all but vanished by night; at 0.28 the night reading ink makes
  // 2.0 : 1 on the page ground, at 0.4 2.86 : 1, and the key drew 0.4 while the figure drew 0.28).
  it('keys a party’s size and number and each line’s width in tier 1, drawn as the figure draws them (P4)', () => {
    const { container } = render(<Graph network={mainnet as FiberNetwork} />);
    const key = within(panel()).getByRole('list', { name: 'Graph key' });
    const party = within(key).getAllByRole('listitem')[0];
    // a circle with a number inside, as a hub carries its count
    expect(party.querySelector('circle')).not.toBeNull();
    expect(party.querySelector('svg text')).toHaveTextContent(/^\d+$/);
    // the key's grey line and every grey edge of the figure: one strength
    const keyGrey = within(key).getAllByRole('listitem')[2].querySelector('line')!;
    const edges = Array.from(
      container.querySelectorAll('svg[data-fiber-graph] [data-edge] line[data-history]')
    );
    expect(edges.length).toBe(35);
    for (const edge of edges)
      expect(edge.getAttribute('stroke-opacity')).toBe(keyGrey.getAttribute('stroke-opacity'));
    expect(keyGrey.getAttribute('stroke-opacity')).toBe('0.4');
    // the key's words are labels: the meta step, the quiet ink (R3)
    expect(key).toHaveClass('text-meta', 'text-text-dim');
  });

  it('says an empty network in the reading ink (R3: an empty state reads)', () => {
    render(<Graph network={{ ...(network as FiberNetwork), parties: [], pairs: [] }} />);
    const empty = screen.getByText('No on-chain parties observed yet.');
    expect(empty).toHaveClass('text-text');
    expect(empty).not.toHaveClass('text-text-dim');
  });

  it('keeps the explainer out of the picture: the column holds the legend until a party is chosen (R13, R14)', () => {
    render(<Graph network={mainnet as FiberNetwork} />);
    expect(panel()).not.toHaveTextContent(/tap or hover/i);
    expect(panel()).not.toHaveTextContent(/Each circle is a party/);
    expect(panel().querySelector('[data-role="party-or-pair"]')).toBeNull();
  });

  it('hovering a party dims everything outside its neighbourhood and shows the party (FF-F3)', () => {
    render(<Graph network={mainnet as FiberNetwork} />);
    fireEvent.mouseEnter(node('ppedrr'));
    expect(node('ppedrr')).toHaveAttribute('data-lit', 'true');
    expect(node('5eyy6y')).toHaveAttribute('data-lit', 'true');
    expect(node('kdeakp')).toHaveAttribute('data-lit', 'false');
    expect(node('kdeakp')).toHaveAttribute('opacity', '0.18');
    expect(within(panel()).getByRole('link', { name: /ppedrr$/ })).toBeInTheDocument();
    expect(within(panel()).getByText(/with 1 party/)).toBeInTheDocument();
    // R2b-b: the party's own state counts, first channel and lock name (FF-F3)
    expect(within(panel()).getByText(/^Default Lock · lock hash/)).toBeInTheDocument();
    for (const label of ['closed together', 'force-closed', 'first channel'])
      expect(within(card()).getByText(label)).toBeInTheDocument();
    // the legend stays while a party is shown
    expect(within(panel()).getByRole('list', { name: 'Graph key' })).toBeInTheDocument();
    fireEvent.mouseLeave(node('ppedrr'));
    expect(node('kdeakp')).toHaveAttribute('data-lit', 'true');
  });

  it('a pair shows open CKB with its unit, lifetime, and block range with separators (FF-F3, FF-F7)', () => {
    const { container } = render(<Graph network={network as FiberNetwork} />);
    const edge = screen.getByRole('button', { name: /^Pair …aaaaaa and …bbbbbb, 2\schannels/ });
    fireEvent.click(edge);
    const p = panel();
    expect(within(p).getByText('…aaaaaa ↔ …bbbbbb')).toBeInTheDocument();
    expect(
      Array.from(p.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
      // a stat in the summary form (readability R5, re-pinned on purpose: was "3,000.00000000 CKB")
    ).toContain('3,000 CKB');
    expect(within(p).getByText('0 settled · 0 not yet')).toBeInTheDocument();
    expect(within(p).getByText('1m 40s')).toBeInTheDocument();
    expect(within(p).getAllByRole('link', { name: '#10' })).toHaveLength(2);
    // R2b-b: first · last opening dates beside the blocks (FF-F3)
    expect(within(p).getByText('2023-11-15 · 2023-11-15')).toBeInTheDocument();
    // the selected pair stays jade/grey as drawn, never repainted grey at 4 px (FF-F7)
    const selected = container.querySelector('[data-edge][data-lit="true"]')!;
    expect(selected.querySelector('[stroke-width="4"]')).toBeNull();
    // visual finish V9: no outline-none — the keyboard focus shows the global jade ring
    expect(edge).toHaveClass('cursor-pointer');
    expect(edge).not.toHaveClass('outline-none');
  });

  // Re-pinned on purpose (lane P4; renamed, it said "says in tier 2 how to read the graph, what a
  // party is, and which channels it cannot draw"): how to read the graph is its key in tier 1, so
  // the tier-2 explainer (FiberGraphNotes, "Each circle is a party …") is gone; what a party is
  // stays in tier 2
  it('says in tier 2 what a party is and which channels it cannot draw', () => {
    render(<FiberPartiesNote network={network as FiberNetwork} />);
    expect(screen.queryByText(/^Each circle is a party/)).toBeNull();
    expect(
      screen.getByText(
        fullText(
          /^Parties are on-chain addresses, not Fiber node ids: the chain shows who funded a channel, not which node ran it\. 1 channel with more or fewer than two parties is not drawn\.$/
        )
      )
    ).toBeInTheDocument();
    // Material pass v2 leftovers: tier-2 prose is set by the shared Prose piece, not by a body
    // class on each paragraph; the parties note is one paragraph set in the page's.
    for (const p of Array.from(document.querySelectorAll('p'))) {
      expect(p).not.toHaveClass('text-body', 'text-text-dim');
    }
  });

  // Material pass v2 leftovers: the graph sits in the instrument frame, and the svg it is drawn in
  // holds everything it draws — so the frame can clip the svg and nothing is lost.
  for (const width of [1200, 358]) {
    it(`sits in the instrument frame and draws nothing outside its box at ${width} px`, () => {
      stubChartWidth(width);
      const { container } = render(<Graph network={mainnet as FiberNetwork} />);
      const svg = container.querySelector('svg[data-fiber-graph]')!;
      const frame = svg.closest('[data-instrument-frame]') as HTMLElement;
      expect(frame).not.toBeNull();
      expect(frame).toHaveAttribute('aria-label', 'The Fiber network on chain');
      // a flat tile since rubbing × Song print: no lit top edge
      expect(frame).toHaveClass('glass');
      expect(frame).not.toHaveClass('neon-edge-top');
      // the detail column stays outside the frame
      expect(frame.contains(panel())).toBe(false);
      // drawn 1:1 at the measured width, clipped to its box (no overflow-visible)
      expect(svg).toHaveAttribute('width', String(width));
      expect(svg.getAttribute('class')).not.toMatch(/overflow-visible/);
      const [, , vw, vh] = svg.getAttribute('viewBox')!.split(' ').map(Number);
      expect(vw).toBe(width);
      expect(String(vh)).toBe(svg.getAttribute('height'));
      const drawing = svg.querySelector('[data-role="drawing"]')!;
      const shift = Number(
        /translate\(0[ ,]+([\d.]+)\)/.exec(drawing.getAttribute('transform')!)![1]
      );
      const nodes = Array.from(svg.querySelectorAll('[data-node]'));
      expect(nodes).toHaveLength(25);
      for (const g of nodes) {
        const hit = g.querySelector('circle[fill="transparent"]')!;
        const r = Number(hit.getAttribute('r')) - 8;
        const cx = Number(hit.getAttribute('cx'));
        const cy = Number(hit.getAttribute('cy')) + shift;
        // its ring (r + 4) and its label (half of six 12 px characters with the halo, rounded up:
        // 24 px; re-pinned from 23 at 11 px)
        const across = Math.max(r + 4, 24);
        expect(cx - across).toBeGreaterThanOrEqual(0);
        expect(cx + across).toBeLessThanOrEqual(vw);
        expect(cy - r - 4).toBeGreaterThanOrEqual(0);
        // the label's baseline 14 px under the circle and its 4 px of descenders
        expect(cy + r + 18).toBeLessThanOrEqual(vh);
        for (const text of Array.from(g.querySelectorAll('text'))) {
          const x = Number(text.getAttribute('x'));
          const y = Number(text.getAttribute('y')) + shift;
          expect(x - 23).toBeGreaterThanOrEqual(0);
          expect(x + 23).toBeLessThanOrEqual(vw);
          expect(y - 11).toBeGreaterThanOrEqual(0);
          expect(y + 4).toBeLessThanOrEqual(vh);
        }
      }
      // the plot's name and the islands' note are inside too
      const name = svg.querySelector('[data-role="chart-name"]')!;
      expect(Number(name.getAttribute('y'))).toBeLessThanOrEqual(vh);
      const note = screen.getByText('not linked to the rest');
      const noteX = Number(note.getAttribute('x'));
      const noteY = Number(note.getAttribute('y')) + shift;
      expect(noteX).toBeGreaterThanOrEqual(0);
      expect(noteX).toBeLessThanOrEqual(vw);
      expect(noteY + 3).toBeLessThanOrEqual(vh);
      // hover linking still works inside the frame
      fireEvent.mouseEnter(node('ppedrr'));
      expect(node('kdeakp')).toHaveAttribute('data-lit', 'false');
      expect(within(panel()).getByRole('link', { name: /ppedrr$/ })).toBeInTheDocument();
      fireEvent.mouseLeave(node('ppedrr'));
    });
  }

  it('draws the graph 1:1 at its measured width, islands below, with 12 px labels (R4, FF-F8)', () => {
    stubChartWidth(358);
    const { container } = render(<Graph network={mainnet as FiberNetwork} />);
    const svg = container.querySelector('svg[data-fiber-graph]')!;
    // laid out for the column, never scaled and never scrolled
    expect(svg).toHaveAttribute('width', '358');
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 358 \d+/);
    expect(svg.getAttribute('height')).toBe(svg.getAttribute('viewBox')!.split(' ')[3]);
    expect(svg.getAttribute('class')).not.toMatch(/min-w-|w-full/);
    expect(container.querySelector('.overflow-x-auto')).toBeNull();
    expect(screen.queryByText('scroll sideways for the whole network')).toBeNull();
    // the islands' note starts a row under the main component
    const note = screen.getByText('not linked to the rest');
    expect(note).toHaveAttribute('text-anchor', 'start');
    expect(note).toHaveAttribute('x', '0');
    // re-pinned (readability R4): the label role is 12 px (was 11)
    expect(note).toHaveAttribute('font-size', '12');
    // party labels are 12 px
    expect(within(svg as HTMLElement).getByText('kdeakp')).toHaveAttribute('font-size', '12');
    for (const n of Array.from(svg.querySelectorAll('[data-node] circle:last-of-type'))) {
      const cx = Number(n.getAttribute('cx'));
      expect(cx).toBeGreaterThan(0);
      expect(cx).toBeLessThan(358);
    }
  });

  it('keeps the in-plot name clear of every party: the drawing moves down when one reaches its band', () => {
    stubChartWidth(358);
    const { container } = render(<Graph network={mainnet as FiberNetwork} />);
    const svg = container.querySelector('svg[data-fiber-graph]')!;
    const drawing = svg.querySelector('[data-role="drawing"]')!;
    const shift = Number(
      /translate\(0[ ,]+([\d.]+)\)/.exec(drawing.getAttribute('transform')!)![1]
    );
    // the name's 12 px line (baseline 14, descent to 17.6) sits in the top 18 px; no party's ring
    // (r + 4) starts above it
    const tops = Array.from(svg.querySelectorAll('[data-node]')).map((g) => {
      const hit = g.querySelector('circle[fill="transparent"]')!;
      const r = Number(hit.getAttribute('r')) - 8;
      return Number(hit.getAttribute('cy')) - r - 4 + shift;
    });
    expect(Math.min(...tops)).toBeGreaterThanOrEqual(18);
    expect(Number(svg.getAttribute('height'))).toBeGreaterThanOrEqual(280);
  });

  it('a tap pins a party’s facts in the line under the graph; a second tap clears it (R6)', () => {
    stubChartWidth(358);
    render(<Graph network={mainnet as FiberNetwork} />);
    // the reading sits under the hero section; its idle line names what it reads, and the
    // section's one "tap or hover" is the reading's own hint (R14)
    expect(screen.getByTestId('under').contains(readout())).toBe(true);
    expect(readout()).toHaveTextContent(`${IDLE}tap or hover`);
    const hints = [screen.getByRole('region', { name: 'hero' }), screen.getByTestId('under')].map(
      (el) => (el.textContent!.match(/tap or hover/gi) ?? []).length
    );
    expect(hints).toEqual([0, 1]);
    // a tap: the emulated mouseenter, then the click
    fireEvent.mouseEnter(node('ppedrr'));
    fireEvent.click(node('ppedrr'));
    fireEvent.mouseLeave(node('ppedrr'));
    expect(readout()).toHaveTextContent(/^selected ppedrr · 3 channels · pairs with 5eyy6y 3/);
    expect(node('ppedrr')).toHaveAttribute('aria-pressed', 'true');
    // the side panel shows the same party
    expect(within(panel()).getByRole('link', { name: /ppedrr$/ })).toBeInTheDocument();
    // the second tap clears the pin even though the emulated hover is still on it
    fireEvent.mouseEnter(node('ppedrr'));
    fireEvent.click(node('ppedrr'));
    expect(readout()).toHaveTextContent(IDLE);
    expect(node('kdeakp')).toHaveAttribute('data-lit', 'true');
  });

  it('pins a pair, lists a hub’s partners by channels, and Escape clears the pin (R6)', () => {
    render(<Graph network={mainnet as FiberNetwork} />);
    fireEvent.click(node('kdeakp'));
    expect(readout()).toHaveTextContent(/^selected kdeakp · 165 channels · pairs with /);
    const partners = readout().textContent!.split('pairs with ')[1].split(', ');
    const counts = partners.map((p) => Number(p.split(' ')[1]));
    expect(counts).toEqual([...counts].sort((a, b) => b - a));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(readout()).toHaveTextContent(IDLE);
    fireEvent.click(
      screen.getAllByRole('button', {
        name: /^Pair …ppedrr and …5eyy6y|^Pair …5eyy6y and …ppedrr/,
      })[0]
    );
    expect(readout()).toHaveTextContent(
      /^selected …(ppedrr|5eyy6y) ↔ …(ppedrr|5eyy6y) · 3 channels/
    );
  });

  it('writes one caption under the reading: one sentence within 640 px (R13, R14)', () => {
    render(<Graph network={mainnet as FiberNetwork} />);
    const caption = screen.getByTestId('under').querySelector('p[data-role="caption"]')!;
    // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[640px])
    expect(caption).toHaveClass('text-body', 'max-w-measure');
    // one line inside 640 px (13 px mono, about 8 px a character)
    // one line of the measure: MEASURE_CHARS, 76 characters at the body step's 8.4 px a
    // glyph (it was length × 8 ≤ 640, which let 77 to 80 characters through)
    expect(caption.textContent!.length).toBeLessThanOrEqual(MEASURE_CHARS);
    expect(caption.textContent).toMatch(/^Each circle is a party, each line a pair/);
  });
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
// of the graph — its name, a party's label under its circle and ring, a hub's count inside its
// circle, the islands' note — stands whole inside the svg, touches no other word, and a party's
// label clears its own circle and ring, measured by the type modules (0.6 em a glyph; a tight line
// over its baseline, its descent under it). The real case is mainnet's network (25 parties), at
// the desktop column (880), the 640 column (584) and on phones (358, 326).
describe('FiberNetworkGraph: every word in its place (readability F3)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it.each([880, 584, 358, 326])('at %i px', (width) => {
    stubChartWidth(width);
    const { container } = render(<Graph network={mainnet as FiberNetwork} />);
    const svg = container.querySelector('svg[data-fiber-graph]')!;
    const lines = allTextLines(svg);
    for (const line of lines) expectInside(line, viewBox(svg), 'the graph', 'flush');
    expectApart(lines, `the graph at ${width}`);
    for (const g of [...svg.querySelectorAll('[data-node]')]) {
      const circles = [...g.querySelectorAll('circle')].filter(
        (c) => c.getAttribute('fill') !== 'transparent'
      );
      // its circle or its ring, with its stroke
      const reach = Math.max(
        ...circles.map(
          (c) => Number(c.getAttribute('r')) + Number(c.getAttribute('stroke-width')) / 2
        )
      );
      const [label] = textLines(g.querySelector('text')!);
      const cy =
        Number(circles[0].getAttribute('cy')) +
        Number(
          /translate\(0[ ,]+([\d.]+)\)/.exec(
            svg.querySelector('[data-role="drawing"]')!.getAttribute('transform')!
          )![1]
        );
      expect(label.top, `"${label.text}" clears its ring`).toBeGreaterThan(cy + reach);
    }
  });
});
