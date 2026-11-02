import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '../utils/test-utils';
import { truncateHash } from '@/lib/utils';
import { PeersTable } from '@/app/network/nodes-table';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

const REACHABLE_PEER_ID = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const UNVERIFIED_PEER_ID = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const DIRECT_ONLY_PEER_ID = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';

describe('PeersTable', () => {
  it('renders exact peer states and candidate-only metadata as em dashes', async () => {
    render(<PeersTable />);

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    expect(screen.getByText(truncateHash(REACHABLE_PEER_ID))).toBeInTheDocument();
    expect(screen.getByText(truncateHash(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    const reachableRow = screen
      .getByText(truncateHash(REACHABLE_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(reachableRow!).getByText('Same-network Identify')).toBeInTheDocument();
    expect(screen.queryByText(/^Unreachable$/)).not.toBeInTheDocument();

    const candidateRow = screen
      .getByText(truncateHash(UNVERIFIED_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(candidateRow!).getByText('Aliases exhausted')).toBeInTheDocument();
    expect(candidateRow).toHaveTextContent('—');
    expect(candidateRow).not.toHaveTextContent('Unknown');
  });

  it('refetches with the selected exact display state', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    fireEvent.change(screen.getByLabelText('Filter by crawler dial state'), {
      target: { value: 'advertisedUnverified' },
    });

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(1));
    expect(screen.getByText(truncateHash(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    expect(screen.queryByText(truncateHash(REACHABLE_PEER_ID))).not.toBeInTheDocument();
  });

  it('offers all six typed observations and refetches with the selected value', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

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
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

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
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    fireEvent.click(screen.getByRole('button', { name: `Show evidence for ${REACHABLE_PEER_ID}` }));

    expect(await screen.findByText('Retained verification')).toBeInTheDocument();
    expect(screen.getByText('Valid Nodes replies: 1')).toBeInTheDocument();
    expect(screen.getByText('GetNodes responses: 1')).toBeInTheDocument();
    expect(screen.getByText('Announce messages: 0')).toBeInTheDocument();
    expect(screen.getByText('Normalized advertisements: 2')).toBeInTheDocument();
    expect(screen.getByText('Rejected advertisements: 0')).toBeInTheDocument();
    expect(screen.getByText('Malformed messages: 0')).toBeInTheDocument();
    expect(screen.getByText('Unexpected messages: 0')).toBeInTheDocument();
    expect(screen.queryByText(/consecutive exhausted rounds/)).not.toBeInTheDocument();
  });

  it('shows addressless direct participation, direction, and crawler dial state independently', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    const directRow = screen
      .getByText(truncateHash(DIRECT_ONLY_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(directRow!).getByText('Direct CKB session')).toBeInTheDocument();
    expect(within(directRow!).getByText('peer → observer')).toBeInTheDocument();
    expect(within(directRow!).getByText('Not dialed by this crawler')).toBeInTheDocument();
    expect(directRow).toHaveTextContent('—');

    fireEvent.click(
      screen.getByRole('button', { name: `Show evidence for ${DIRECT_ONLY_PEER_ID}` })
    );
    expect(await screen.findByText('Direct CKB session evidence')).toBeInTheDocument();
    const evidence = screen.getByTestId('peer-evidence-row');
    expect(
      within(evidence).getByText(/session address metadata: none reported/)
    ).toBeInTheDocument();
    expect(within(evidence).getByText(/not used as crawler dial aliases/)).toBeInTheDocument();
    expect(within(evidence).getByText('No completed probe observation')).toBeInTheDocument();
  });
});

/**
 * Responsive plan T3 (board T3-peer-evidence.dc.html, review A#5/A#6): the evidence table needs
 * 2,144 px, more than any page column (1,248 px at 1280). Below the measured width where every
 * column fits a row is peer id + dial alias + dial state, and the "▸" expander holds the rest.
 */
describe('PeersTable form by measured width (T3)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const COLUMN_LABELS = [
    'Peer ID',
    'Dial alias',
    'Participation evidence',
    'Session initiation',
    'Crawler dial result',
    'Version',
    'Country',
    'Latest evidence',
    'Last dial',
    'Last verified',
    'RTT',
  ];

  it('folds each peer to its id, dial alias and dial state where the columns do not fit', async () => {
    stubChartWidth(1248);
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
    // No column header row in the folded form.
    expect(screen.queryByText('Participation evidence')).toBeNull();

    const row = screen.getAllByTestId('peer-row')[0];
    const toggle = within(row).getByRole('button', {
      name: `Show evidence for ${REACHABLE_PEER_ID}`,
    });
    // The whole row is the expander, a 44 px target.
    expect(toggle).toHaveClass('min-h-[44px]', 'w-full');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(toggle).toHaveTextContent('▸');
    expect(toggle).toHaveTextContent(truncateHash(REACHABLE_PEER_ID));
    expect(toggle).toHaveTextContent('/ip4/1.2.3.4/tcp/8115');
    expect(within(toggle).getByText('Same-network Identify')).toBeInTheDocument();
    // Every other column waits in the expander.
    expect(row).not.toHaveTextContent('0.114.0');
    expect(row).not.toHaveTextContent('United States');
    expect(row).not.toHaveTextContent('42 ms');
    expect(row).not.toHaveTextContent('Identify ·');
  });

  // Table-rows T8: where the row has the room (from 560 px, the wide phone-row rule), the folded
  // peer is one line — id, dial alias cut to fit (whole in its title and the expander), the dial
  // state as a row tag; on a phone the alias keeps its own line. The dial state is a kind, so a
  // RowTag (accent for a same-network Identify) in every form, also the full table.
  it('folds each peer to one line where it fits, its dial state a row tag (T8)', async () => {
    stubChartWidth(1248);
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const [reachable, candidate, undialed] = screen.getAllByTestId('peer-row');
    const toggle = within(reachable).getByRole('button', {
      name: `Show evidence for ${REACHABLE_PEER_ID}`,
    });
    expect(toggle).toHaveClass('grid-cols-[14px_auto_minmax(0,1fr)_auto]', 'items-center');
    const alias = within(toggle).getByText('/ip4/1.2.3.4/tcp/8115');
    // As wide as its words, at most its column (R16: a text box is its words), cut to fit.
    expect(alias).toHaveClass('truncate', 'justify-self-start', 'max-w-full');
    expect(alias).toHaveAttribute('title', '/ip4/1.2.3.4/tcp/8115');
    expect(alias).not.toHaveClass('col-span-2');
    const tag = within(toggle).getByText('Same-network Identify');
    expect(tag).toHaveAttribute('data-row-tag', 'accent');
    expect(within(candidate).getByText('Aliases exhausted')).toHaveAttribute(
      'data-row-tag',
      'ghost'
    );
    expect(within(undialed).getByText('Not dialed by this crawler')).toHaveAttribute(
      'data-row-tag',
      'ghost'
    );
    expect(document.querySelector('.bg-positive\\/10')).toBeNull();
  });

  it('keeps the alias on its own line in a phone-wide fold, the tag allowed to wrap', async () => {
    stubChartWidth(358);
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const toggle = within(screen.getAllByTestId('peer-row')[0]).getByRole('button', {
      name: `Show evidence for ${REACHABLE_PEER_ID}`,
    });
    expect(toggle).toHaveClass('grid-cols-[14px_auto_minmax(0,1fr)]', 'min-h-[44px]');
    expect(within(toggle).getByText('/ip4/1.2.3.4/tcp/8115')).toHaveClass('col-span-2');
    const tag = within(toggle).getByText('Same-network Identify');
    expect(tag).toHaveAttribute('data-row-tag', 'accent');
    expect(tag).toHaveClass('whitespace-normal');
  });

  it('opens the expander onto every column as labelled pairs, the multiaddr wrapping', async () => {
    stubChartWidth(358);
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const row = screen.getAllByTestId('peer-row')[0];
    const toggle = within(row).getByRole('button', {
      name: `Show evidence for ${REACHABLE_PEER_ID}`,
    });
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(toggle).toHaveTextContent('▾');

    const pairs = within(row).getByTestId('peer-columns');
    const labels = [...pairs.querySelectorAll('dt')].map((dt) => dt.textContent);
    expect(labels).toEqual(COLUMN_LABELS);
    const value = (label: string) => {
      const dt = [...pairs.querySelectorAll('dt')].find((d) => d.textContent === label)!;
      return dt.nextElementSibling as HTMLElement;
    };
    expect(value('Peer ID')).toHaveTextContent(REACHABLE_PEER_ID);
    expect(value('Dial alias')).toHaveTextContent('/ip4/1.2.3.4/tcp/8115');
    expect(value('Dial alias')).toHaveClass('[overflow-wrap:anywhere]');
    expect(value('Dial alias')).not.toHaveClass('truncate');
    expect(value('Participation evidence')).toHaveTextContent('Identify');
    expect(value('Session initiation')).toHaveTextContent('—');
    expect(value('Crawler dial result')).toHaveTextContent('Same-network Identify');
    expect(value('Version')).toHaveTextContent('0.114.0');
    expect(value('Country')).toHaveTextContent('United States');
    expect(value('RTT')).toHaveTextContent('42 ms');
    // The retained evidence still loads under the pairs.
    expect(await screen.findByText('Retained verification')).toBeInTheDocument();
  });

  it('keeps the full table where every column fits (2,144 px)', async () => {
    stubChartWidth(2144);
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    for (const label of COLUMN_LABELS) expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getAllByTestId('peer-row')[0]).toHaveTextContent('0.114.0');
    expect(
      within(screen.getAllByTestId('peer-row')[0]).getByText('Same-network Identify')
    ).toHaveAttribute('data-row-tag', 'accent');
    expect(screen.queryByTestId('peer-columns')).toBeNull();
    expect(document.querySelector('.overflow-x-auto')).toBeNull();
  });

  it('folds the four filter controls into one "filters" button below md', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const button = screen.getByRole('button', { name: /^filters/ });
    expect(button).toHaveClass('md:hidden');
    expect(button).toHaveAttribute('aria-expanded', 'false');
    const panel = document.getElementById(button.getAttribute('aria-controls')!)!;
    expect(panel).toHaveClass('hidden', 'md:flex');
    for (const label of [
      'Filter by crawler dial state',
      'Filter by address observation',
      'Filter by country',
      'Filter by version',
    ])
      expect(within(panel).getByLabelText(label)).toBeInTheDocument();

    fireEvent.click(button);
    expect(button).toHaveAttribute('aria-expanded', 'true');
    expect(panel).toHaveClass('flex');
    expect(panel).not.toHaveClass('hidden');

    // A filter in force is counted on the folded button.
    fireEvent.change(within(panel).getByLabelText('Filter by country'), {
      target: { value: 'US' },
    });
    expect(button).toHaveTextContent('filters · 1');
  });
});
