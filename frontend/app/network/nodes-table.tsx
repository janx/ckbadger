'use client';

import { Fragment, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import { TerminalRow } from '@/components/ui/terminal-panel';
import { RowTag, type RowTagVariant } from '@/components/ui/row-tag';
import { WIDE_ROW_MIN_WIDTH } from '@/components/ui/list-table';
import { DetailSection } from '@/components/ui/detail-layout';
import { Pager } from '@/components/ui/pager';
import { Select, type SelectOption } from '@/components/ui/select';
import { TextField } from '@/components/ui/text-field';
import { useElementWidth } from '@/hooks/useElementWidth';
import {
  api,
  type NetworkAddressProbeEvidence,
  type NetworkAddressProbeResult,
  type NetworkCompletedPeerEvidence,
  type NetworkPeerDetail,
  type NetworkPeerDisplayState,
  type NetworkPeerSummary,
  type NetworkSessionInitiator,
} from '@/lib/api';
import { cn, formatTimeAgo } from '@/lib/utils';
import { countNoun, formatCount } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';
import { ClockText } from '@/components/ui/utc8-date';
import { unitNode } from '@/components/ui/units';

interface PeerFilters {
  state?: NetworkPeerDisplayState;
  observation?: NetworkAddressProbeResult;
  country?: string;
  version?: string;
}

const GRID_TEMPLATE = '13rem 15rem 18rem 14rem 17rem 7rem 9rem 9rem 8rem 8rem 5rem';
/** 132rem: the eleven columns and their gaps. */
const GRID_MIN_PX = 2112;
/** The grid plus the header row's padding: the width the table needs (2,144 px). */
const TABLE_MIN_PX = GRID_MIN_PX + 32;
const COLUMNS = [
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

/** The crawler's dial states, in the filter's order. */
const DIAL_STATES: readonly NetworkPeerDisplayState[] = [
  'reachable',
  'verifiedUnavailable',
  'advertisedUnverified',
  'foreignNetwork',
  'noCompletedObservation',
];

/** The typed address observations, in the filter's order. */
const OBSERVATIONS: readonly NetworkAddressProbeResult[] = [
  'dialRequestFailed',
  'noAuthenticatedSessionBeforeDeadline',
  'authenticatedSessionWithoutIdentifyBeforeDeadline',
  'malformedIdentify',
  'foreignNetwork',
  'sameNetworkIdentified',
];

/**
 * The filters' choices, `all` (no filter: the empty value) first, as the board's select writes it,
 * then each state in the words the rows use for it: a dial state as its row's badge says it
 * (crawlerDialStateTag), an observation as the evidence says it (observationLabel). Every choice
 * holds a 360 px phone's line alone in its well: 37 glyphs at 13 px (291 px of 328 less the well's
 * border and the select's padding).
 */
const STATE_OPTIONS: SelectOption<'' | NetworkPeerDisplayState>[] = [
  { value: '', label: 'all' },
  ...DIAL_STATES.map((state) => ({ value: state, label: crawlerDialStateTag(state).label })),
];

const OBSERVATION_OPTIONS: SelectOption<'' | NetworkAddressProbeResult>[] = [
  { value: '', label: 'all' },
  ...OBSERVATIONS.map((result) => ({ value: result, label: observationLabel(result) })),
];

function orDash(value: string | null | undefined): string {
  return value != null && value.trim().length > 0 ? value : '—';
}

/** A crawler time (seconds) as its age, read from the page's one clock (lane E4). */
function ago(seconds: number): ReactNode {
  return <ClockText text={(now) => formatTimeAgo(seconds * 1000, now)} />;
}

function optionalAge(timestamp: number | null): ReactNode {
  return timestamp == null ? '—' : ago(timestamp);
}

/**
 * The crawler's dial state, a kind, as a row tag (table-rows T8): accent for a completed
 * same-network Identify, plain for a verification seen before or a foreign network, set back
 * (ghost) where nothing was verified. The dial-state filter offers these same words.
 */
function crawlerDialStateTag(state: NetworkPeerDisplayState): {
  label: string;
  variant: RowTagVariant;
} {
  switch (state) {
    case 'reachable':
      return { label: 'Same-network Identify', variant: 'accent' };
    case 'verifiedUnavailable':
      // the aliases of the latest round exhausted; a same-network Identify verified before is
      // retained (35 glyphs; "· prior verification" took 38, one past a phone's select)
      return { label: 'Aliases exhausted · verified before', variant: 'default' };
    case 'advertisedUnverified':
      return { label: 'Aliases exhausted', variant: 'ghost' };
    case 'foreignNetwork':
      return { label: 'Foreign-network Identify', variant: 'default' };
    case 'noCompletedObservation':
      return { label: 'Not dialed by this crawler', variant: 'ghost' };
  }
}

/**
 * A typed address observation in the evidence's words, which the observation filter offers too.
 * The two that end at the probe's deadline say "in time" (before it): no authenticated session
 * by then (32 glyphs), or a session authenticated and no Identify by then (34); the words that
 * kept "deadline" took 40 and 54, and "Authenticated, no Identify by deadline" 38, past the 37 a
 * phone's select holds.
 */
function observationLabel(result: NetworkAddressProbeResult): string {
  switch (result) {
    case 'dialRequestFailed':
      return 'Dial request failed';
    case 'noAuthenticatedSessionBeforeDeadline':
      return 'No authenticated session in time';
    case 'authenticatedSessionWithoutIdentifyBeforeDeadline':
      return 'Authenticated, no Identify in time';
    case 'malformedIdentify':
      return 'Malformed Identify';
    case 'foreignNetwork':
      return 'Foreign network';
    case 'sameNetworkIdentified':
      return 'Same-network Identify completed';
  }
}

function outcomeLabel(outcome: NetworkCompletedPeerEvidence['outcome']): string {
  switch (outcome) {
    case 'sameNetworkIdentified':
      return 'Same-network Identify completed';
    case 'exhausted':
      return 'All advertised aliases exhausted';
    case 'foreignNetwork':
      return 'Foreign network identified';
  }
}

function vantageLabel(vantage: string): string {
  return vantage === 'configuredLocalCkbRpcObserverAndThisCrawler'
    ? 'configured local CKB RPC observer + this crawler'
    : vantage;
}

function sessionInitiatorLabel(initiator: NetworkSessionInitiator): string {
  return initiator === 'peerInitiated' ? 'peer → observer' : 'observer → peer';
}

function ObservationList({ observations }: { observations: NetworkAddressProbeEvidence[] }) {
  if (observations.length === 0) {
    return <p className="text-text-dim">No address observations recorded.</p>;
  }
  return (
    <ul className="space-y-1">
      {observations.map((observation, index) => (
        <li key={`${observation.address}-${observation.observedAt}-${index}`} className="text-text">
          <span className="break-all">{observation.address}</span>
          <span className="text-text-dim">
            {' '}
            · <span>{observationLabel(observation.result)}</span> · {observation.elapsedMs} ms ·
            observed {ago(observation.observedAt)}
          </span>
        </li>
      ))}
    </ul>
  );
}

function EvidenceDetail({ detail }: { detail: NetworkPeerDetail }) {
  return (
    <div className="border-base-border bg-base-bg/70 text-small space-y-4 border-t px-4 py-4 font-mono">
      <div className="flex flex-wrap gap-x-6 gap-y-1">
        <span className="text-text-dim">
          Observation vantage:{' '}
          <span className="text-text">{vantageLabel(detail.observationVantage)}</span>
        </span>
        <span className="text-text-dim">
          First advertised:{' '}
          <span className="text-text">{optionalAge(detail.firstDiscoveredAt)}</span>
        </span>
        <span className="text-text-dim">
          Latest positive evidence:{' '}
          <span className="text-text">{ago(detail.latestPositiveObservedAt)}</span>
        </span>
      </div>

      <div>
        <h4 className="text-text-bright mb-1 font-bold">Advertised aliases</h4>
        {detail.aliases.length === 0 ? (
          <p className="text-text-dim">No aliases recorded.</p>
        ) : (
          <ul className="space-y-1">
            {detail.aliases.map((alias) => (
              <li key={alias.address} className="text-text">
                {/* the address alone breaks anywhere; the words after it break between them */}
                <span className="break-all">{alias.address}</span>{' '}
                <span className="text-text-dim">
                  · first advertised {ago(alias.firstAdvertisedAt)} · last advertised{' '}
                  {ago(alias.lastAdvertisedAt)} · last verified {optionalAge(alias.lastVerifiedAt)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div>
        <h4 className="text-text-bright mb-1 font-bold">Direct CKB session evidence</h4>
        {detail.directSessions.length === 0 ? (
          <p className="text-text-dim">No retained direct-session observation.</p>
        ) : (
          <ul className="space-y-1">
            {detail.directSessions.map((session) => (
              <li key={`${session.observerPeerId}-${session.initiator}`} className="text-text-dim">
                <span className="text-text">{sessionInitiatorLabel(session.initiator)}</span> ·
                observer{' '}
                <span className="text-aqua" title={session.observerPeerId}>
                  <ShortId value={session.observerPeerId} kind="hash" size="row" />
                </span>{' '}
                · {session.clientVersion || 'unknown client'} · observed{' '}
                {session.observationCount.toLocaleString('en-US')} completed{' '}
                {countNoun(session.observationCount, 'round')} · latest{' '}
                {ago(session.lastObservedAt)} · session address metadata:{' '}
                {session.sessionAddresses.length === 0
                  ? 'none reported'
                  : session.sessionAddresses.join(', ')}
              </li>
            ))}
          </ul>
        )}
        <p className="text-text-dim max-w-measure mt-1">
          Direction is from the configured observer&apos;s vantage. Session addresses are not used
          as crawler dial aliases.
        </p>
      </div>

      {detail.lastCompleted ? (
        <div>
          <h4 className="text-text-bright mb-1 font-bold">
            Last completed round #{detail.lastCompleted.roundId}
          </h4>
          <p className="text-text-dim max-w-measure mb-1">
            <span>{outcomeLabel(detail.lastCompleted.outcome)}</span>
            {detail.lastCompleted.outcome === 'exhausted' && (
              <>
                {' '}
                ·{' '}
                <span>
                  {detail.lastCompleted.consecutiveExhaustedRounds} consecutive exhausted rounds
                </span>
              </>
            )}
          </p>
          <ObservationList observations={detail.lastCompleted.observations} />
        </div>
      ) : (
        <p className="text-text-dim">No completed probe observation.</p>
      )}

      {detail.active && (
        <div>
          <h4 className="text-text-bright mb-1 font-bold">Active round #{detail.active.roundId}</h4>
          <p className="text-text-dim max-w-measure">State: {detail.active.state}</p>
          <ObservationList observations={detail.active.observations} />
        </div>
      )}

      {detail.verified && (
        <div>
          <h4 className="text-text-bright mb-1 font-bold">Retained verification</h4>
          <p className="text-text-dim max-w-measure">
            {detail.verified.clientVersion} · last same-network Identify{' '}
            {ago(detail.verified.lastReachableAt)} · protocols{' '}
            {detail.verified.protocols.join(', ') || '—'}
          </p>
          <h5 className="text-text-bright mb-1 mt-2 font-bold">Discovery</h5>
          <ul className="text-text-dim grid gap-x-6 gap-y-1 sm:grid-cols-2">
            <li>Valid Nodes replies: {detail.verified.discovery.validNodesMessages}</li>
            <li>GetNodes responses: {detail.verified.discovery.validResponseMessages}</li>
            <li>Announce messages: {detail.verified.discovery.validAnnounceMessages}</li>
            <li>
              Normalized advertisements: {detail.verified.discovery.normalizedAdvertisedAddresses}
            </li>
            <li>
              Rejected advertisements: {detail.verified.discovery.rejectedAdvertisedAddresses}
            </li>
            <li>Malformed messages: {detail.verified.discovery.malformedMessages}</li>
            <li>Unexpected messages: {detail.verified.discovery.unexpectedMessages}</li>
          </ul>
        </div>
      )}

      <div>
        <h4 className="text-text-bright mb-1 font-bold">Advertised by</h4>
        {detail.advertisers.length === 0 ? (
          <p className="text-text-dim">No retained advertiser evidence.</p>
        ) : (
          <ul className="space-y-1">
            {detail.advertisers.map((advertiser) => (
              <li
                key={`${advertiser.advertiserPeerId}-${advertiser.alias}`}
                className="text-text-dim"
              >
                <span className="text-aqua" title={advertiser.advertiserPeerId}>
                  <ShortId value={advertiser.advertiserPeerId} kind="hash" size="row" />
                </span>{' '}
                · <span className="break-all">{advertiser.alias}</span> · observed{' '}
                {unitNode(formatCount(advertiser.observationCount, 'completed round'))} · first
                round #{advertiser.firstObservedRound} · latest round #
                {advertiser.lastObservedRound} · {ago(advertiser.lastObservedAt)}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

/** One peer's eleven column values, in `COLUMNS` order: the table's cells and the folded pairs. */
function peerColumns(peer: NetworkPeerSummary): Array<{
  label: string;
  value: ReactNode;
  /** The cell in the table row. */
  cell: string;
  /** The value in the folded row's labelled pairs. */
  pair?: string;
  title?: string;
}> {
  const state = crawlerDialStateTag(peer.crawlerDialState);
  const participation = [
    peer.participation.discoveryAdvertised ? 'Discovery ad' : null,
    peer.participation.directSessionObserved ? 'Direct CKB session' : null,
    peer.participation.crawlerIdentified ? 'Identify' : null,
  ].filter((value): value is string => value != null);
  return [
    {
      label: 'Peer ID',
      value: peer.peerId,
      cell: 'text-aqua truncate font-mono text-small',
      pair: 'text-aqua [overflow-wrap:anywhere]',
    },
    {
      label: 'Dial alias',
      value: orDash(peer.primaryAddr),
      cell: 'text-text truncate font-mono text-small',
      pair: '[overflow-wrap:anywhere]',
      title: peer.primaryAddr ?? undefined,
    },
    {
      label: 'Participation evidence',
      value: participation.join(' · ') || '—',
      cell: 'text-text-dim truncate text-small',
      title: participation.join(' · '),
    },
    {
      label: 'Session initiation',
      value: peer.sessionInitiators.map(sessionInitiatorLabel).join(' · ') || '—',
      cell: 'text-text-dim truncate font-mono text-small',
    },
    {
      label: 'Crawler dial result',
      value: <RowTag variant={state.variant}>{state.label}</RowTag>,
      cell: '',
    },
    {
      label: 'Version',
      value: orDash(peer.version),
      cell: 'text-text truncate font-mono text-small',
    },
    {
      label: 'Country',
      value: orDash(peer.country),
      cell: 'text-text truncate text-small',
      title: peer.country ?? undefined,
    },
    {
      label: 'Latest evidence',
      value: ago(peer.latestPositiveObservedAt),
      cell: 'text-text-dim text-small',
    },
    {
      label: 'Last dial',
      value: optionalAge(peer.lastDialObservedAt),
      cell: 'text-text-dim text-small',
    },
    {
      label: 'Last verified',
      value: optionalAge(peer.lastReachableAt),
      cell: 'text-text-dim text-small',
    },
    {
      label: 'RTT',
      value: peer.rttMs == null ? '—' : `${peer.rttMs} ms`,
      cell: 'text-text-dim text-right font-mono text-small tabular-nums',
    },
  ];
}

function PeerEvidenceRow({ peerId }: { peerId: string }) {
  const detail = useQuery({
    queryKey: ['network', 'peer', peerId],
    queryFn: () => api.getNetworkPeer(peerId),
  });
  return (
    <TerminalRow data-testid="peer-evidence-row">
      {isAwaited(detail) ? (
        <div className="text-text-dim text-small px-4 py-3 font-mono">Loading evidence…</div>
      ) : detail.isError || !detail.data ? (
        <div className="text-negative text-small px-4 py-3 font-mono">
          Failed to load peer evidence.
        </div>
      ) : (
        <EvidenceDetail detail={detail.data} />
      )}
    </TerminalRow>
  );
}

/**
 * One peer. Where the eleven columns fit, a table row; otherwise (board T3-peer-evidence.dc.html)
 * the row is the peer id, its dial alias and its dial state, the whole row a 44 px expander, and
 * opening it lists every column as a labelled pair — the multiaddr wrapping, never cut — above
 * the retained evidence. `oneLine` (table-rows T8, from `WIDE_ROW_MIN_WIDTH`): the three on one
 * line, the alias cut to fit (whole in its title and in the expander); below it, the alias keeps a
 * line of its own.
 */
function PeerRow({
  peer,
  folded,
  oneLine,
}: {
  peer: NetworkPeerSummary;
  folded: boolean;
  oneLine: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const columns = peerColumns(peer);
  const state = crawlerDialStateTag(peer.crawlerDialState);
  const toggle = {
    onClick: () => setExpanded((value) => !value),
    title: peer.peerId,
    'aria-expanded': expanded,
    'aria-label': `${expanded ? 'Hide' : 'Show'} evidence for ${peer.peerId}`,
  };

  if (folded)
    return (
      <Fragment>
        <TerminalRow data-testid="peer-row" className="px-0 py-0">
          {oneLine ? (
            <button
              type="button"
              {...toggle}
              className="grid min-h-[44px] w-full grid-cols-[14px_auto_minmax(0,1fr)_auto] items-center gap-x-3 px-1 py-2 text-left font-mono sm:px-3"
            >
              <span aria-hidden="true" className="text-text-dim text-small">
                {expanded ? '▾' : '▸'}
              </span>
              {/* The one-line row is drawn from 560 px of container, at every desktop width (the
                  table needs 2,144): the row size, the evidence lines' (R9); 17 characters at
                  12 px, 122.4 px, the address beside it cut to what is left. */}
              <span className="text-aqua text-small whitespace-nowrap">
                <ShortId value={peer.peerId} kind="hash" size="row" />
              </span>
              {/* As wide as its words (R16), at most the column, cut to fit. */}
              <span
                className="text-text text-meta min-w-0 max-w-full justify-self-start truncate"
                title={peer.primaryAddr ?? undefined}
              >
                {orDash(peer.primaryAddr)}
              </span>
              <RowTag variant={state.variant}>{state.label}</RowTag>
            </button>
          ) : (
            <button
              type="button"
              {...toggle}
              className="grid min-h-[44px] w-full grid-cols-[14px_auto_minmax(0,1fr)] items-start gap-x-2 gap-y-0.5 px-1 py-2 text-left font-mono sm:px-3"
            >
              <span aria-hidden="true" className="text-text-dim text-small">
                {expanded ? '▾' : '▸'}
              </span>
              {/* The id keeps its width (already cut in the middle); the dial state takes what is
                  left of the line and wraps inside its tag only when it must. */}
              <span className="text-aqua text-small whitespace-nowrap">
                <ShortId value={peer.peerId} kind="hash" size="tight" />
              </span>
              <span className="min-w-0 justify-self-end">
                <RowTag variant={state.variant} className="whitespace-normal">
                  {state.label}
                </RowTag>
              </span>
              <span
                className="text-text text-meta col-span-2 col-start-2 min-w-0 max-w-full justify-self-start truncate"
                title={peer.primaryAddr ?? undefined}
              >
                {orDash(peer.primaryAddr)}
              </span>
            </button>
          )}
          {expanded && (
            <dl
              data-testid="peer-columns"
              className="text-meta grid grid-cols-[minmax(0,7.5rem)_minmax(0,1fr)] gap-x-3 gap-y-1 pb-3 pl-[26px] pr-1 font-mono sm:pl-[34px] sm:pr-3 lg:grid-cols-[7.5rem_minmax(0,1fr)_7.5rem_minmax(0,1fr)]"
            >
              {columns.map((c) => (
                <Fragment key={c.label}>
                  <dt className="text-text-dim">{c.label}</dt>
                  <dd className={cn('text-text min-w-0', c.pair)}>{c.value}</dd>
                </Fragment>
              ))}
            </dl>
          )}
        </TerminalRow>
        {expanded && <PeerEvidenceRow peerId={peer.peerId} />}
      </Fragment>
    );

  return (
    <Fragment>
      <TerminalRow data-testid="peer-row">
        <div
          className="grid w-full items-center gap-x-4"
          style={{ gridTemplateColumns: GRID_TEMPLATE, minWidth: GRID_MIN_PX }}
        >
          <button
            type="button"
            {...toggle}
            className="text-aqua text-small truncate text-left font-mono hover:underline"
          >
            {expanded ? '▾ ' : '▸ '}
            <ShortId value={peer.peerId} kind="hash" size="row" />
          </button>
          {columns.slice(1).map((c) => (
            <span key={c.label} className={c.cell} title={c.title}>
              {c.value}
            </span>
          ))}
        </div>
      </TerminalRow>
      {expanded && <PeerEvidenceRow peerId={peer.peerId} />}
    </Fragment>
  );
}

/**
 * Five peers first (hierarchy R5, a list on a topic page), then fifty more a page: every peer stays
 * one "next 50 ↓" away, and the cursor is a peer id, so the page sizes may differ.
 */
const FIRST_PEERS = 5;
const MORE_PEERS = 50;

export function PeersTable() {
  const [filters, setFilters] = useState<PeerFilters>({});
  const [cursor, setCursor] = useState<string | undefined>();
  const [rows, setRows] = useState<NetworkPeerSummary[]>([]);
  const mergedTokenRef = useRef<string | null>(null);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const filtersId = useId();
  const [boxRef, boxWidth] = useElementWidth<HTMLDivElement>();
  // The measured width decides (docs/RESPONSIVE_LAYOUT.md rule 4): the table only where all
  // eleven columns fit, never a sideways scroller. Until measured, the table is assumed.
  const folded = boxWidth !== null && boxWidth < TABLE_MIN_PX;
  // Table-rows T8: a folded peer is one line where the row has the wide phone row's room.
  const oneLine = boxWidth !== null && boxWidth >= WIDE_ROW_MIN_WIDTH;

  const read = useQuery({
    queryKey: ['network', 'peers', filters, cursor],
    queryFn: () =>
      api.getNetworkPeers({ ...filters, cursor, limit: cursor ? MORE_PEERS : FIRST_PEERS }),
  });
  const { data, isError, isFetching } = read;

  useEffect(() => {
    if (!data) return;
    const token = cursor ?? '__first__';
    if (mergedTokenRef.current === token) return;
    mergedTokenRef.current = token;
    setRows((previous) => (cursor ? [...previous, ...data.items] : data.items));
  }, [data, cursor]);

  function applyFilters(next: PeerFilters) {
    setFilters(next);
    setCursor(undefined);
    setRows([]);
    mergedTokenRef.current = null;
  }

  // awaited, not isLoading: a read paused offline or between retries has not answered "no peers"
  const showSkeleton = isAwaited(read) && rows.length === 0;
  const showEmpty = !isAwaited(read) && !isError && rows.length === 0;
  const activeFilters = [
    filters.state,
    filters.observation,
    filters.country,
    filters.version,
  ].filter((value) => value !== undefined).length;

  return (
    // One heading — the section's "Peer evidence" — and plain rules, not a terminal panel (CH-56).
    // Below md the four filter controls fold into one "filters" button beside it (board T3).
    <DetailSection
      title="Peer evidence"
      actions={
        <button
          type="button"
          aria-expanded={filtersOpen}
          aria-controls={filtersId}
          onClick={() => setFiltersOpen((open) => !open)}
          className="border-base-border bg-base-surface text-text hover:text-text-bright text-small h-8 border px-2.5 font-mono md:hidden"
        >
          filters{activeFilters > 0 ? ` · ${activeFilters}` : ''}{' '}
          <span aria-hidden="true" className="text-text-dim">
            {filtersOpen ? '▴' : '▾'}
          </span>
        </button>
      }
    >
      <div aria-busy={isFetching} ref={boxRef} className="border-row-rule border-y">
        <div>
          <div
            id={filtersId}
            className={cn(
              'border-base-border flex-wrap items-center gap-3 border-b px-4 py-2',
              filtersOpen ? 'flex' : 'hidden md:flex'
            )}
          >
            {/* The controls board's select after its label and its text field in the well; each
                filter starts the list again from its first five. On a phone the select, its
                label and its well shrink to the panel's line (every box min-w-0): the label 18
                characters at 13 px (140.4 px) and 10 px, the well what is left of 296 px at 360. */}
            <Select
              label="crawler dial state"
              value={filters.state ?? ''}
              options={STATE_OPTIONS}
              onChange={(value) => applyFilters({ ...filters, state: value || undefined })}
            />
            <Select
              label="address observation"
              value={filters.observation ?? ''}
              options={OBSERVATION_OPTIONS}
              onChange={(value) => applyFilters({ ...filters, observation: value || undefined })}
            />
            <TextField
              label="Filter by country"
              placeholder="country"
              value={filters.country ?? ''}
              onChange={(value) => applyFilters({ ...filters, country: value || undefined })}
              className="w-[200px]"
            />
            <TextField
              label="Filter by version"
              placeholder="version"
              value={filters.version ?? ''}
              onChange={(value) => applyFilters({ ...filters, version: value || undefined })}
              className="w-[200px]"
            />
          </div>

          <div>
            {!folded && (
              <div
                className="border-row-rule bg-base-surface/50 text-text-dim text-small grid items-center gap-x-4 border-b px-4 py-2 font-mono"
                style={{ gridTemplateColumns: GRID_TEMPLATE, minWidth: GRID_MIN_PX }}
              >
                {COLUMNS.map((label, index) => (
                  <div key={label} className={index === COLUMNS.length - 1 ? 'text-right' : ''}>
                    {label}
                  </div>
                ))}
              </div>
            )}

            {showSkeleton ? (
              <div className="text-text-dim text-body py-12 text-center font-mono">
                Loading peer evidence…
              </div>
            ) : isError ? (
              <div className="text-negative text-body py-12 text-center font-mono">
                Failed to load peer evidence.
              </div>
            ) : showEmpty ? (
              <div className="text-text-dim text-body py-12 text-center font-mono">
                No peers match these evidence filters.
              </div>
            ) : (
              rows.map((peer) => (
                <PeerRow key={peer.peerId} peer={peer} folded={folded} oneLine={oneLine} />
              ))
            )}
          </div>
        </div>

        {/* The controls board's short-list form: "next 50 ↓", the peers the next load asks for
            (the API sends no total and no page size; the cursor is the last peer shown). */}
        {data?.nextCursor != null && (
          <div className="border-row-rule flex justify-center border-t px-4 py-2">
            <Pager
              mode="more"
              next={{ count: MORE_PEERS, onShow: () => setCursor(data.nextCursor ?? undefined) }}
            />
          </div>
        )}
      </div>
    </DetailSection>
  );
}
