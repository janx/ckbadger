import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
import { FiberNetworkGraph } from '@/components/fiber/fiber-network-graph';
import network from '../fixtures/fiber-network.json';
import type { FiberNetwork } from '@/lib/api';

describe('FiberNetworkGraph', () => {
  it('selects an exact observed pair without inventing an address or payment topology', () => {
    render(<FiberNetworkGraph network={network as FiberNetwork} />);
    expect(screen.getByText(/do not show off-chain routes/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Pair .*2 channels/ }));
    expect(screen.getByText('2 channels ever · 1 open')).toBeInTheDocument();
    expect(screen.getAllByText(/Unknown address/)).toHaveLength(2);
    expect(screen.getByText('3,000.00000000')).toBeInTheDocument();
  });
});
