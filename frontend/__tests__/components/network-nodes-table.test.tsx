import { describe, it, expect } from 'vitest';
import { fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { truncateHash } from '@/lib/utils';
import { NodesTable } from '@/app/network/nodes-table';

// peerIds mirror the canned two-node page served by the global MSW handler
// (frontend/__tests__/msw/handlers.ts). Node A is reachable, Node B is not.
const REACHABLE_PEER_ID = 'QmReachablePeer1111111111111111111111111111AaBb';
const UNREACHABLE_PEER_ID = 'QmUnreachablePeer22222222222222222222222222CcDd';

describe('NodesTable', () => {
  it('renders both nodes from the canned page (truncated peerId, version, country, reachable badge)', async () => {
    render(<NodesTable />);

    // Both rows render.
    await waitFor(() => expect(screen.getAllByTestId('node-row')).toHaveLength(2));

    // peerIds are truncated via the shared hash-truncate util.
    expect(screen.getByText(truncateHash(REACHABLE_PEER_ID))).toBeInTheDocument();
    expect(screen.getByText(truncateHash(UNREACHABLE_PEER_ID))).toBeInTheDocument();
    // Full peerId is not rendered (proves truncation actually happened).
    expect(screen.queryByText(REACHABLE_PEER_ID)).not.toBeInTheDocument();

    // Version + country cells render as real DOM text.
    expect(screen.getByText('0.114.0')).toBeInTheDocument();
    expect(screen.getByText('0.113.0')).toBeInTheDocument();
    expect(screen.getByText('United States')).toBeInTheDocument();
    expect(screen.getByText('Germany')).toBeInTheDocument();

    // Reachability badges (one of each).
    expect(screen.getByText('Reachable')).toBeInTheDocument();
    expect(screen.getByText('Unreachable')).toBeInTheDocument();
  });

  it('refetches with reachable=true when the "Reachable only" filter is toggled, leaving one row', async () => {
    render(<NodesTable />);

    // Start with both nodes.
    await waitFor(() => expect(screen.getAllByTestId('node-row')).toHaveLength(2));

    // Toggle the reachable-only filter — this changes the query key and triggers a real refetch;
    // the MSW handler honours reachable=true and returns only the reachable node.
    fireEvent.click(screen.getByRole('button', { name: 'Reachable only' }));

    // Now only the reachable node remains.
    await waitFor(() => expect(screen.getAllByTestId('node-row')).toHaveLength(1));
    expect(screen.getByText('0.114.0')).toBeInTheDocument();
    expect(screen.getByText(truncateHash(REACHABLE_PEER_ID))).toBeInTheDocument();
    // The unreachable node dropped out of the refetched page.
    expect(screen.queryByText('0.113.0')).not.toBeInTheDocument();
    expect(screen.queryByText('Germany')).not.toBeInTheDocument();
    expect(screen.queryByText(truncateHash(UNREACHABLE_PEER_ID))).not.toBeInTheDocument();
  });
});
