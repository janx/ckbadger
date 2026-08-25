import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '../utils/test-utils';
import { truncateHash } from '@/lib/utils';
import { PeersTable } from '@/app/network/nodes-table';

const REACHABLE_PEER_ID = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const UNVERIFIED_PEER_ID = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';

describe('PeersTable', () => {
  it('renders exact peer states and candidate-only metadata as em dashes', async () => {
    render(<PeersTable />);

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(2));

    expect(screen.getByText(truncateHash(REACHABLE_PEER_ID))).toBeInTheDocument();
    expect(screen.getByText(truncateHash(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    const reachableRow = screen
      .getByText(truncateHash(REACHABLE_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(reachableRow!).getByText('Reachable')).toBeInTheDocument();
    expect(screen.queryByText(/^Unreachable$/)).not.toBeInTheDocument();

    const candidateRow = screen
      .getByText(truncateHash(UNVERIFIED_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(candidateRow!).getByText('Advertised · unverified')).toBeInTheDocument();
    expect(candidateRow).toHaveTextContent('—');
    expect(candidateRow).not.toHaveTextContent('Unknown');
  });

  it('refetches with the selected exact display state', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(2));

    fireEvent.change(screen.getByLabelText('Filter by peer state'), {
      target: { value: 'advertisedUnverified' },
    });

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(1));
    expect(screen.getByText(truncateHash(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    expect(screen.queryByText(truncateHash(REACHABLE_PEER_ID))).not.toBeInTheDocument();
  });

  it('offers all six typed observations and refetches with the selected value', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(2));

    const select = screen.getByLabelText('Filter by address observation');
    expect(
      within(select)
        .getAllByRole('option')
        .map((option) => (option as HTMLOptionElement).value)
    ).toEqual([
      '',
      'dialRequestFailed',
      'noAuthenticatedSessionBeforeDeadline',
      'authenticatedSessionWithoutIdentifyBeforeDeadline',
      'malformedIdentify',
      'foreignNetwork',
      'sameNetworkIdentified',
    ]);

    fireEvent.change(select, {
      target: { value: 'noAuthenticatedSessionBeforeDeadline' },
    });

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(1));
    expect(screen.getByText(truncateHash(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    expect(screen.queryByText(truncateHash(REACHABLE_PEER_ID))).not.toBeInTheDocument();
  });

  it('expands a peer into address-level probe evidence', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(2));

    fireEvent.click(
      screen.getByRole('button', { name: `Show evidence for ${UNVERIFIED_PEER_ID}` })
    );

    expect(await screen.findByText('Last completed round #5')).toBeInTheDocument();
    const evidence = screen.getByTestId('peer-evidence-row');
    expect(
      within(evidence).getByText('No authenticated session before deadline')
    ).toBeInTheDocument();
    expect(within(evidence).getByText('2 consecutive exhausted rounds')).toBeInTheDocument();
    expect(within(evidence).getByText(/first advertised/)).toHaveTextContent(/last advertised/);
    expect(within(evidence).getByText('Advertised by')).toBeInTheDocument();
  });

  it('shows retained Discovery evidence for a verified peer', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(2));

    fireEvent.click(screen.getByRole('button', { name: `Show evidence for ${REACHABLE_PEER_ID}` }));

    expect(await screen.findByText('Retained verification')).toBeInTheDocument();
    expect(screen.getByText('Valid Nodes replies: 1')).toBeInTheDocument();
    expect(screen.getByText('Normalized advertisements: 2')).toBeInTheDocument();
    expect(screen.getByText('Rejected advertisements: 0')).toBeInTheDocument();
    expect(screen.getByText('Malformed messages: 0')).toBeInTheDocument();
    expect(screen.getByText('Unexpected messages: 0')).toBeInTheDocument();
    expect(screen.queryByText(/consecutive exhausted rounds/)).not.toBeInTheDocument();
  });
});
