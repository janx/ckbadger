import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen, within } from '../utils/test-utils';
import { FiberNetworkGraph } from '@/components/fiber/fiber-network-graph';
import network from '../fixtures/fiber-network.json';
import mainnet from '../fixtures/fiber-network-mainnet.json';
import type { FiberNetwork } from '@/lib/api';

const node = (tail: string) => screen.getByRole('button', { name: new RegExp(`^Party …${tail},`) });
const panel = () => screen.getByRole('complementary', { name: 'Party or pair' });

describe('FiberNetworkGraph', () => {
  it('draws the network as a graph with the board’s legend and node labels (FF-F1)', () => {
    const { container } = render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    const heading = screen.getByRole('heading', { name: 'The network, as the chain sees it' });
    expect(heading).toHaveClass('text-[15px]');
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
    expect(screen.getByText('not linked to the rest')).toBeInTheDocument();
    // tier 1 carries no developer sentence about represented histories (FF-F6)
    expect(screen.queryByText(/channel histories represented/)).toBeNull();
  });

  it('hovering a party dims everything outside its neighbourhood and shows the party (FF-F3)', () => {
    render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    expect(within(panel()).getByText('Hover a party or a pair')).toBeInTheDocument();
    fireEvent.mouseEnter(node('ppedrr'));
    expect(node('ppedrr')).toHaveAttribute('data-lit', 'true');
    expect(node('5eyy6y')).toHaveAttribute('data-lit', 'true');
    expect(node('kdeakp')).toHaveAttribute('data-lit', 'false');
    expect(node('kdeakp')).toHaveAttribute('opacity', '0.18');
    expect(within(panel()).getByRole('link', { name: /ppedrr$/ })).toBeInTheDocument();
    expect(within(panel()).getByText(/with 1 party/)).toBeInTheDocument();
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

  it('keeps nodes legible at phone width by scrolling the graph inside its own box (FF-F8)', () => {
    const { container } = render(<FiberNetworkGraph network={mainnet as FiberNetwork} />);
    const svg = container.querySelector('svg[data-fiber-graph]')!;
    expect(svg).toHaveClass('min-w-[640px]');
    expect(svg.parentElement).toHaveClass('overflow-x-auto');
  });
});
