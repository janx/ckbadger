import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '../utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { FiberNetworkGraph } from '@/components/fiber/fiber-network-graph';
import network from '../fixtures/fiber-network.json';
import mainnet from '../fixtures/fiber-network-mainnet.json';
import type { FiberNetwork } from '@/lib/api';

const node = (tail: string) => screen.getByRole('button', { name: new RegExp(`^Party …${tail},`) });
const panel = () => screen.getByRole('complementary', { name: 'Party or pair' });

describe('FiberNetworkGraph', () => {
  // The graph is laid out for its measured column (R4); 880 px is its desktop column at 1280.
  beforeEach(() => stubChartWidth(880));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws the network as a graph with the board’s legend and node labels (FF-F1)', () => {
    const { container } = render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    const heading = screen.getByRole('heading', { name: 'The network, as the chain sees it' });
    expect(heading).toHaveClass('text-section');
    for (const legend of [
      'CKB locked now',
      'channels over time',
      'force-closed',
      'has an open channel',
    ])
      expect(screen.getByText(legend)).toBeInTheDocument();
    const svg = container.querySelector('svg[data-fiber-graph]')!;
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

  it('hovering a party dims everything outside its neighbourhood and shows the party (FF-F3)', () => {
    render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    expect(within(panel()).getByText('Tap or hover a party or a pair')).toBeInTheDocument();
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
      expect(within(panel()).getByText(label)).toBeInTheDocument();
    fireEvent.mouseLeave(node('ppedrr'));
    expect(node('kdeakp')).toHaveAttribute('data-lit', 'true');
  });

  it('a pair shows open CKB with its unit, lifetime, and block range with separators (FF-F3, FF-F7)', () => {
    const { container } = render(<FiberNetworkGraph network={network as FiberNetwork} />);
    const edge = screen.getByRole('button', { name: /^Pair …aaaaaa and …bbbbbb, 2 channels/ });
    fireEvent.click(edge);
    const p = panel();
    expect(within(p).getByText('…aaaaaa ↔ …bbbbbb')).toBeInTheDocument();
    expect(
      Array.from(p.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toContain('3,000.00000000 CKB');
    expect(within(p).getByText('0 settled · 0 not yet')).toBeInTheDocument();
    expect(within(p).getByText('1 m 40 s')).toBeInTheDocument();
    expect(within(p).getAllByRole('link', { name: '#10' })).toHaveLength(2);
    // R2b-b: first · last opening dates beside the blocks (FF-F3)
    expect(within(p).getByText('2023-11-15 · 2023-11-15')).toBeInTheDocument();
    // the selected pair stays jade/grey as drawn, never repainted grey at 4 px (FF-F7)
    const selected = container.querySelector('[data-edge][data-lit="true"]')!;
    expect(selected.querySelector('[stroke-width="4"]')).toBeNull();
    expect(edge).toHaveClass('outline-none');
  });

  it('says plainly when some channels cannot be drawn as a pair', () => {
    render(<FiberNetworkGraph network={network as FiberNetwork} />);
    expect(
      screen.getByText(/ 1 channel with more or fewer than two parties is not drawn\.$/)
    ).toBeInTheDocument();
  });

  it('draws the graph 1:1 at its measured width, islands below, with 11 px labels (R4, FF-F8)', () => {
    stubChartWidth(358);
    const { container } = render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
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
    expect(note).toHaveAttribute('font-size', '11');
    // party labels are 11 px
    expect(within(svg as HTMLElement).getByText('kdeakp')).toHaveAttribute('font-size', '11');
    for (const n of Array.from(svg.querySelectorAll('[data-node] circle:last-of-type'))) {
      const cx = Number(n.getAttribute('cx'));
      expect(cx).toBeGreaterThan(0);
      expect(cx).toBeLessThan(358);
    }
  });

  it('a tap pins a party’s facts in the line under the graph; a second tap clears it (R6)', () => {
    stubChartWidth(358);
    render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    const readout = () => screen.getByRole('status', { name: 'Party or pair readout' });
    expect(readout()).toHaveTextContent('Tap or hover a party or a pair.');
    expect(readout()).toHaveTextContent('tap or hover');
    // a tap: the emulated mouseenter, then the click
    fireEvent.mouseEnter(node('ppedrr'));
    fireEvent.click(node('ppedrr'));
    fireEvent.mouseLeave(node('ppedrr'));
    expect(readout()).toHaveTextContent(/^tapped ppedrr · 3 channels · pairs with 5eyy6y 3/);
    expect(node('ppedrr')).toHaveAttribute('aria-pressed', 'true');
    // the side panel shows the same party
    expect(within(panel()).getByRole('link', { name: /ppedrr$/ })).toBeInTheDocument();
    // the second tap clears the pin even though the emulated hover is still on it
    fireEvent.mouseEnter(node('ppedrr'));
    fireEvent.click(node('ppedrr'));
    expect(readout()).toHaveTextContent('Tap or hover a party or a pair.');
    expect(node('kdeakp')).toHaveAttribute('data-lit', 'true');
  });

  it('pins a pair, lists a hub’s partners by channels, and Escape clears the pin (R6)', () => {
    render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    const readout = () => screen.getByRole('status', { name: 'Party or pair readout' });
    fireEvent.click(node('kdeakp'));
    expect(readout()).toHaveTextContent(/^tapped kdeakp · 165 channels · pairs with /);
    const partners = readout().textContent!.split('pairs with ')[1].split(', ');
    const counts = partners.map((p) => Number(p.split(' ')[1]));
    expect(counts).toEqual([...counts].sort((a, b) => b - a));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(readout()).toHaveTextContent('Tap or hover a party or a pair.');
    fireEvent.click(
      screen.getAllByRole('button', {
        name: /^Pair …ppedrr and …5eyy6y|^Pair …5eyy6y and …ppedrr/,
      })[0]
    );
    expect(readout()).toHaveTextContent(/^tapped …(ppedrr|5eyy6y) ↔ …(ppedrr|5eyy6y) · 3 channels/);
  });
});
