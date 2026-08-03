import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, useLocation } from 'react-router-dom';
import userEvent from '@testing-library/user-event';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { ProposalGraphRenderer } from '@/components/proposal-graph-renderer';
import type { GraphNode, ProposalGraphMetadata } from '@/lib/api';

// The shared test setup replaces `@/src/navigation` with a no-op router; these
// tests assert the real network-prefixing behaviour, so restore the actual module.
vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

interface ForceGraphStubProps {
  onNodeClick: (node: { nodeType: string; data: { blockNumber: number } }) => void;
}

// Stand in for the lazily loaded force graph: a button that fires the renderer's
// node-click handler with a commit-block node.
vi.mock('@/lib/dynamic-client', () => ({
  default: () =>
    function ForceGraphStub({ onNodeClick }: ForceGraphStubProps) {
      return (
        <button
          type="button"
          onClick={() => onNodeClick({ nodeType: 'commit_block', data: { blockNumber: 12345 } })}
        >
          commit-block-node
        </button>
      );
    },
}));

const nodes: GraphNode[] = [
  {
    id: 'block-12345',
    nodeType: 'commit_block',
    label: '#12345',
    data: { blockNumber: 12345 },
  },
];

const metadata: ProposalGraphMetadata = {
  sourceBlock: 12340,
  totalProposals: 1,
  committedCount: 1,
  commitmentWindow: {
    close: 2,
    far: 10,
    earliestCommitBlock: 12342,
    latestCommitBlock: 12350,
  },
};

function Harness() {
  const location = useLocation();

  return (
    <>
      <div data-testid="pathname">{location.pathname}</div>
      <ProposalGraphRenderer nodes={nodes} links={[]} metadata={metadata} />
    </>
  );
}

describe('ProposalGraphRenderer block navigation', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('navigates to the block under the active network prefix', async () => {
    const user = userEvent.setup();

    render(
      <MemoryRouter initialEntries={['/testnet/blocks/12340']}>
        <Harness />
      </MemoryRouter>
    );

    await user.click(screen.getByRole('button', { name: 'commit-block-node' }));

    await waitFor(() => {
      // A bare `/blocks/12345` would be resolved against the DEFAULT network by
      // the route guard, 404-ing a testnet-only block.
      expect(screen.getByTestId('pathname').textContent).toBe('/testnet/blocks/12345');
    });
  });
});
