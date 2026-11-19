import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { onlineManager } from '@tanstack/react-query';
import { act } from '@testing-library/react';
import { onTestFinished } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '../utils/test-utils';
import { PeersTable } from '@/app/network/nodes-table';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { loadUnderStillClock, tick } from '@/__tests__/utils/still-clock';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { containerContentPx, insetsPx, textRunPx } from '@/__tests__/utils/tailwind-box';
import { NARROWEST_LINE_PX } from '@/lib/control-row-fit';

const REACHABLE_PEER_ID = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const UNVERIFIED_PEER_ID = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const DIRECT_ONLY_PEER_ID = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
/** A peer id as the table writes it (U-09): the row size in the table, the tight size folded. */
const rowId = (id: string) => `${id.slice(0, 10)}…${id.slice(-6)}`;
const tightId = (id: string) => `${id.slice(0, 6)}…${id.slice(-4)}`;

describe('PeersTable', () => {
  it('renders exact peer states and candidate-only metadata as em dashes', async () => {
    render(<PeersTable />);

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    expect(screen.getByText(rowId(REACHABLE_PEER_ID))).toBeInTheDocument();
    expect(screen.getByText(rowId(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    const reachableRow = screen
      .getByText(rowId(REACHABLE_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(reachableRow!).getByText('Same-network Identify')).toBeInTheDocument();
    expect(screen.queryByText(/^Unreachable$/)).not.toBeInTheDocument();

    const candidateRow = screen
      .getByText(rowId(UNVERIFIED_PEER_ID))
      .closest<HTMLElement>('[data-testid]');
    expect(within(candidateRow!).getByText('Aliases exhausted')).toBeInTheDocument();
    expect(candidateRow).toHaveTextContent('—');
    expect(candidateRow).not.toHaveTextContent('Unknown');
  });

  it('refetches with the selected exact display state', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    // re-pinned (lane C2 topics): the controls board's select, named by the label it stands after
    // (it was a bare select named "Filter by crawler dial state")
    fireEvent.change(screen.getByLabelText('crawler dial state'), {
      target: { value: 'advertisedUnverified' },
    });

    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(1));
    expect(screen.getByText(rowId(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    expect(screen.queryByText(rowId(REACHABLE_PEER_ID))).not.toBeInTheDocument();
  });

  it('offers all six typed observations and refetches with the selected value', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    // re-pinned (lane C2 topics): the select after its label "address observation", its first
    // choice the board's "all" (it was a bare select named "Filter by address observation",
    // opening on "All address observations")
    const select = screen.getByLabelText('address observation');
    expect(within(select).getAllByRole('option')[0]).toHaveTextContent(/^all$/);
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
    expect(screen.getByText(rowId(UNVERIFIED_PEER_ID))).toBeInTheDocument();
    expect(screen.queryByText(rowId(REACHABLE_PEER_ID))).not.toBeInTheDocument();
  });

  // Lane C2 (topics), written before "Load more" and the filters are replaced: five peers first,
  // then fifty more a load by the cursor the API served, under the ones shown; a filter starts the
  // list again from its first five.
  it('loads more peers by the served cursor under the first, and a filter starts again from the first five', async () => {
    const asked: { cursor: string | null; limit: string | null; state: string | null }[] = [];
    const peer = (id: string) => ({
      peerId: id.repeat(64),
      crawlerDialState: 'reachable',
      participation: {
        discoveryAdvertised: false,
        directSessionObserved: false,
        crawlerIdentified: true,
      },
      sessionInitiators: [],
      primaryAddr: '/ip4/1.2.3.4/tcp/8115',
      version: '0.114.0',
      country: 'United States',
      asn: null,
      lastAdvertisedAt: 1751500800,
      lastDialObservedAt: 1751500800,
      latestPositiveObservedAt: 1751500800,
      lastReachableAt: 1751500800,
      rttMs: 42,
    });
    server.use(
      http.get('/api/:network/v1/network/peers', ({ request }) => {
        const params = new URL(request.url).searchParams;
        asked.push({
          cursor: params.get('cursor'),
          limit: params.get('limit'),
          state: params.get('state'),
        });
        return params.get('cursor') === 'after-d'
          ? HttpResponse.json({ items: [peer('e')], nextCursor: null })
          : HttpResponse.json({ items: [peer('d')], nextCursor: 'after-d' });
      })
    );
    // re-pinned (lane C2 topics): the controls board's short-list form, "next 50 ↓", and the
    // select after its label (they were the button "Load more" and a bare select named "Filter by
    // crawler dial state")
    const more = () => screen.getByRole('button', { name: 'next 50 ↓' });
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(1));
    expect(asked).toEqual([{ cursor: null, limit: '5', state: null }]);
    fireEvent.click(more());
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(2));
    expect(asked.at(-1)).toEqual({ cursor: 'after-d', limit: '50', state: null });
    // the first page's peer stays, the next one under it, and the list has no more to load
    const [first, next] = screen.getAllByTestId('peer-row');
    expect(within(first).getByText(rowId('d'.repeat(64)))).toBeInTheDocument();
    expect(within(next).getByText(rowId('e'.repeat(64)))).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'next 50 ↓' })).toBeNull();
    fireEvent.change(screen.getByLabelText('crawler dial state'), {
      target: { value: 'reachable' },
    });
    await waitFor(() =>
      expect(asked.at(-1)).toEqual({ cursor: null, limit: '5', state: 'reachable' })
    );
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(1));
    expect(screen.queryByText(rowId('e'.repeat(64)))).toBeNull();
  });

  // The coordinator's Chrome check (lane C2 topics): a native select is as wide as its widest
  // option and, where its well is narrower, clips its value without a mark, and a clipped state is
  // another state. Every choice of both filters holds the 328 px line of a 360 px phone alone in
  // its well, well and all (where the pair does not fit, the select wraps under its label): the
  // well's border, the select's padding and the glyphs of lib/type-metrics.ts at the select's
  // 13 px, 291 px for the words, 37 glyphs.
  it('fits every choice of both filters in a 360 px phone’s line, alone in its well', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const over: string[] = [];
    for (const name of ['crawler dial state', 'address observation']) {
      const select = screen.getByLabelText<HTMLSelectElement>(name);
      const chrome = insetsPx(select.parentElement!, 360).box + insetsPx(select, 360).box;
      expect(chrome, name).toBe(2 + 35);
      for (const option of Array.from(select.options)) {
        const px = Math.round((chrome + textRunPx(option.textContent!, select, 360)) * 100) / 100;
        if (px > NARROWEST_LINE_PX) over.push(`${name}: "${option.textContent}" ${px} px`);
      }
    }
    expect(over).toEqual([]);
  });

  // Lane c2-finish, rulings round 1, item 3: the filter panel's 16 px side padding left its
  // content 296 px of a 360 px phone's 328, and both wells are wider (310 and 302.2 px), so they
  // stood 14 and 6.2 px into the padding. Below md the panel's content takes the page's line, its
  // own side padding 0 (16 px from md, where the panel stands beside nothing else in the box).
  it('gives the filters the page’s line below md: each well whole inside it at 360', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const wells: Record<string, number> = {};
    for (const name of ['crawler dial state', 'address observation']) {
      const select = screen.getByLabelText<HTMLSelectElement>(name);
      const panel = select.parentElement!.parentElement!.parentElement!;
      expect(
        [360, 390, 640, 768, 1280].map((w) => insetsPx(panel, w).box),
        name
      ).toEqual([0, 0, 0, 32, 32]);
      const line = containerContentPx(360) - insetsPx(panel, 360).box;
      expect(line).toBe(NARROWEST_LINE_PX);
      const chrome = insetsPx(select.parentElement!, 360).box + insetsPx(select, 360).box;
      wells[name] = Math.max(
        ...Array.from(
          select.options,
          (o) => Math.round((chrome + textRunPx(o.textContent!, select, 360)) * 100) / 100
        )
      );
      expect(wells[name], name).toBeLessThanOrEqual(line);
    }
    // the widest choices: 'Aliases exhausted · verified before' (35 glyphs), 'Authenticated, no
    // Identify in time' (34), at 7.8 px a glyph and 37 px of frame and padding
    expect(wells).toEqual({ 'crawler dial state': 310, 'address observation': 302.2 });
  });

  // A filter says what the rows say: the dial-state filter names a state as the row's badge does,
  // the observation filter an observation as the evidence does, so a reader finds the choice by
  // the words in the row.
  it('names a dial state as the row’s badge does, an observation as the evidence does', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    const choices = (name: string) =>
      Array.from(screen.getByLabelText<HTMLSelectElement>(name).options, (o) => o.textContent);
    const badges = Array.from(document.querySelectorAll('[data-row-tag]'), (t) => t.textContent);
    expect(badges).toEqual([
      'Same-network Identify',
      'Aliases exhausted',
      'Not dialed by this crawler',
    ]);
    for (const badge of badges) expect(choices('crawler dial state')).toContain(badge);
    fireEvent.click(
      screen.getByRole('button', { name: `Show evidence for ${UNVERIFIED_PEER_ID}` })
    );
    // the last round's observations: each address, then its result in the evidence's words
    const round = (await screen.findByText('Last completed round #5')).parentElement!;
    const observed = Array.from(round.querySelectorAll('li > span > span'), (s) => s.textContent);
    expect(observed.length).toBeGreaterThan(0);
    for (const words of observed) expect(choices('address observation')).toContain(words);
  });

  it('expands a peer into address-level probe evidence', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));

    fireEvent.click(
      screen.getByRole('button', { name: `Show evidence for ${UNVERIFIED_PEER_ID}` })
    );

    expect(await screen.findByText('Last completed round #5')).toBeInTheDocument();
    const evidence = screen.getByTestId('peer-evidence-row');
    // re-pinned (lane C2 topics): the observation in words that hold a phone's select, which
    // offers the same words (it was "No authenticated session before deadline", 40 glyphs)
    expect(within(evidence).getByText('No authenticated session in time')).toBeInTheDocument();
    expect(within(evidence).getByText('2 consecutive exhausted rounds')).toBeInTheDocument();
    expect(within(evidence).getByText(/first advertised/)).toHaveTextContent(/last advertised/);
    expect(within(evidence).getByText('Advertised by')).toBeInTheDocument();
  });

  // Lane E4: every age the table writes (a row's latest evidence, last dial and last verified; the
  // evidence's observations, aliases, sessions and advertisers) reads the page's one clock, so it
  // moves with no data change; each was written once at render. However many there are, the page
  // runs one timer, and none once it is gone.
  it('moves every age with no data change, on one timer for all of them (E4)', async () => {
    // the served observations are at 1,751,500,800 s (2025-07-03 00:00 UTC): the clock stands 20 s on
    vi.useFakeTimers({ now: 1_751_500_820_000 });
    onTestFinished(() => {
      vi.useRealTimers();
    });
    const { unmount } = render(<PeersTable />);
    await loadUnderStillClock(() => screen.queryAllByTestId('peer-row').length === 3);
    const row = screen.getByText(rowId(REACHABLE_PEER_ID)).closest<HTMLElement>('[data-testid]')!;
    // latest evidence, last dial, last verified
    expect(within(row).getAllByText('20s ago')).toHaveLength(3);
    fireEvent.click(screen.getByRole('button', { name: `Show evidence for ${REACHABLE_PEER_ID}` }));
    await loadUnderStillClock(() => screen.queryByText('Retained verification') !== null);
    const evidence = screen.getByTestId('peer-evidence-row');
    const ages = () => within(evidence).getAllByText(/\b20s ago\b|\b21s ago\b|\b1m ago\b/).length;
    const shown = ages();
    expect(shown).toBeGreaterThan(2);
    expect(evidence).toHaveTextContent(/observed 20s ago/);
    expect(evidence).toHaveTextContent(/last same-network Identify 20s ago/);
    // thirteen ages on the table, the three rows' 3 + 2 + 1 and the evidence's 7: one clock, one
    // timer
    const readers = (document.body.textContent ?? '').match(
      /\d+[smhdy] ago|this device's clock is behind/g
    );
    expect(readers).toHaveLength(13);
    expect(vi.getTimerCount()).toBe(1);
    tick(1000);
    expect(within(row).getAllByText('21s ago')).toHaveLength(3);
    expect(evidence).toHaveTextContent(/observed 21s ago/);
    expect(evidence).toHaveTextContent(/last same-network Identify 21s ago/);
    expect(evidence).not.toHaveTextContent(/\b20s ago/);
    tick(39_000);
    expect(within(row).getAllByText('1m ago')).toHaveLength(3);
    expect(evidence).toHaveTextContent(/last same-network Identify 1m ago/);
    expect(ages()).toBe(shown);
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
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
      .getByText(rowId(DIRECT_ONLY_PEER_ID))
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
    expect(within(evidence).getByText('No completed probe observation.')).toBeInTheDocument();
  });

  // A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
  // awaited: the list says it is loading, never "No peers match", and a peer's evidence never
  // "Failed to load"; both draw once back online.
  it('says the list is loading while its read is paused, then lists the peers once back online', async () => {
    const { resume } = renderPaused(<PeersTable />);
    expect(await screen.findByText('Loading peer evidence…')).toBeInTheDocument();
    expect(screen.queryByText('No peers match these evidence filters.')).toBeNull();
    resume();
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
  });

  it('says a peer’s evidence is loading while its read is paused, then shows it once back online', async () => {
    render(<PeersTable />);
    await waitFor(() => expect(screen.getAllByTestId('peer-row')).toHaveLength(3));
    onlineManager.setOnline(false);
    onTestFinished(() => onlineManager.setOnline(true));
    fireEvent.click(
      screen.getByRole('button', { name: `Show evidence for ${UNVERIFIED_PEER_ID}` })
    );
    expect(await screen.findByText('Loading evidence…')).toBeInTheDocument();
    expect(screen.queryByText('Failed to load peer evidence.')).toBeNull();
    act(() => onlineManager.setOnline(true));
    expect(await screen.findByText('Last completed round #5')).toBeInTheDocument();
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
    expect(toggle).toHaveTextContent(tightId(REACHABLE_PEER_ID));
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
    // re-pinned (lane C2 topics): the two selects are named by the labels they stand after (they
    // were "Filter by crawler dial state" and "Filter by address observation"); the two fields
    // keep their names, the board's text field has no visible label
    for (const label of [
      'crawler dial state',
      'address observation',
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
