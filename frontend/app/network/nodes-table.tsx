'use client';

import { Fragment, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { TerminalRow } from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { DetailSection } from '@/components/ui/detail-layout';
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
import { cn, formatTimeAgo, truncateHash } from '@/lib/utils';

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

const STATE_OPTIONS: Array<{ value: NetworkPeerDisplayState; label: string }> = [
  { value: 'reachable', label: 'Same-network Identify completed' },
  { value: 'verifiedUnavailable', label: 'Latest aliases exhausted · previously verified' },
  { value: 'advertisedUnverified', label: 'Aliases exhausted · no retained verification' },
  { value: 'foreignNetwork', label: 'Foreign-network Identify' },
  { value: 'noCompletedObservation', label: 'Not dialed by this crawler' },
];

const OBSERVATION_OPTIONS: Array<{ value: NetworkAddressProbeResult; label: string }> = [
  { value: 'dialRequestFailed', label: 'Dial request failed' },
  {
    value: 'noAuthenticatedSessionBeforeDeadline',
    label: 'No authenticated session before deadline',
  },
  {
    value: 'authenticatedSessionWithoutIdentifyBeforeDeadline',
    label: 'Authenticated session without Identify before deadline',
  },
  { value: 'malformedIdentify', label: 'Malformed Identify' },
  { value: 'foreignNetwork', label: 'Foreign network' },
  { value: 'sameNetworkIdentified', label: 'Same-network Identify completed' },
];

function orDash(value: string | null | undefined): string {
  return value != null && value.trim().length > 0 ? value : '—';
}

function optionalAge(timestamp: number | null): string {
  return timestamp == null ? '—' : formatTimeAgo(timestamp * 1000);
}

function crawlerDialStateBadge(state: NetworkPeerDisplayState): {
  label: string;
  variant: 'green' | 'neutral' | 'blue' | 'red' | 'gray';
} {
  switch (state) {
    case 'reachable':
      return { label: 'Same-network Identify', variant: 'green' };
    case 'verifiedUnavailable':
      return { label: 'Aliases exhausted · prior verification', variant: 'neutral' };
    case 'advertisedUnverified':
      return { label: 'Aliases exhausted', variant: 'gray' };
    case 'foreignNetwork':
      return { label: 'Foreign-network Identify', variant: 'red' };
    case 'noCompletedObservation':
      return { label: 'Not dialed by this crawler', variant: 'blue' };
  }
}

function observationLabel(result: NetworkAddressProbeResult): string {
  switch (result) {
    case 'dialRequestFailed':
      return 'Dial request failed';
    case 'noAuthenticatedSessionBeforeDeadline':
      return 'No authenticated session before deadline';
    case 'authenticatedSessionWithoutIdentifyBeforeDeadline':
      return 'Authenticated session without Identify before deadline';
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
    return <p className="text-text-dim">No address observations recorded</p>;
  }
  return (
    <ul className="space-y-1">
      {observations.map((observation, index) => (
        <li key={`${observation.address}-${observation.observedAt}-${index}`} className="text-text">
          <span className="break-all">{observation.address}</span>
          <span className="text-text-dim">
            {' '}
            · <span>{observationLabel(observation.result)}</span> · {observation.elapsedMs} ms ·
            observed {formatTimeAgo(observation.observedAt * 1000)}
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
          <span className="text-text">{formatTimeAgo(detail.latestPositiveObservedAt * 1000)}</span>
        </span>
      </div>

      <div>
        <h4 className="text-text-bright mb-1 font-bold">Advertised aliases</h4>
        {detail.aliases.length === 0 ? (
          <p className="text-text-dim">No aliases recorded</p>
        ) : (
          <ul className="space-y-1">
            {detail.aliases.map((alias) => (
              <li key={alias.address} className="text-text break-all">
                {alias.address}{' '}
                <span className="text-text-dim">
                  · first advertised {formatTimeAgo(alias.firstAdvertisedAt * 1000)} · last
                  advertised {formatTimeAgo(alias.lastAdvertisedAt * 1000)} · last verified{' '}
                  {optionalAge(alias.lastVerifiedAt)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div>
        <h4 className="text-text-bright mb-1 font-bold">Direct CKB session evidence</h4>
        {detail.directSessions.length === 0 ? (
          <p className="text-text-dim">No retained direct-session observation</p>
        ) : (
          <ul className="space-y-1">
            {detail.directSessions.map((session) => (
              <li key={`${session.observerPeerId}-${session.initiator}`} className="text-text-dim">
                <span className="text-text">{sessionInitiatorLabel(session.initiator)}</span> ·
                observer{' '}
                <span className="text-aqua" title={session.observerPeerId}>
                  {truncateHash(session.observerPeerId)}
                </span>{' '}
                · {session.clientVersion || 'unknown client'} · observed {session.observationCount}{' '}
                completed round{session.observationCount === 1 ? '' : 's'} · latest{' '}
                {formatTimeAgo(session.lastObservedAt * 1000)} · session address metadata:{' '}
                {session.sessionAddresses.length === 0
                  ? 'none reported'
                  : session.sessionAddresses.join(', ')}
              </li>
            ))}
          </ul>
        )}
        <p className="text-text-dim mt-1">
          Direction is from the configured observer&apos;s vantage. Session addresses are not used
          as crawler dial aliases.
        </p>
      </div>

      {detail.lastCompleted ? (
        <div>
          <h4 className="text-text-bright mb-1 font-bold">
            Last completed round #{detail.lastCompleted.roundId}
          </h4>
          <p className="text-text-dim mb-1">
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
        <p className="text-text-dim">No completed probe observation</p>
      )}

      {detail.active && (
        <div>
          <h4 className="text-text-bright mb-1 font-bold">Active round #{detail.active.roundId}</h4>
          <p className="text-text-dim">State: {detail.active.state}</p>
          <ObservationList observations={detail.active.observations} />
        </div>
      )}

      {detail.verified && (
        <div>
          <h4 className="text-text-bright mb-1 font-bold">Retained verification</h4>
          <p className="text-text-dim">
            {detail.verified.clientVersion} · last same-network Identify{' '}
            {formatTimeAgo(detail.verified.lastReachableAt * 1000)} · protocols{' '}
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
          <p className="text-text-dim">No retained advertiser evidence</p>
        ) : (
          <ul className="space-y-1">
            {detail.advertisers.map((advertiser) => (
              <li
                key={`${advertiser.advertiserPeerId}-${advertiser.alias}`}
                className="text-text-dim"
              >
                <span className="text-aqua" title={advertiser.advertiserPeerId}>
                  {truncateHash(advertiser.advertiserPeerId)}
                </span>{' '}
                · <span className="break-all">{advertiser.alias}</span> · observed{' '}
                {advertiser.observationCount} completed round
                {advertiser.observationCount === 1 ? '' : 's'} · first round #
                {advertiser.firstObservedRound} · latest round #{advertiser.lastObservedRound} ·{' '}
                {formatTimeAgo(advertiser.lastObservedAt * 1000)}
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
  const state = crawlerDialStateBadge(peer.crawlerDialState);
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
      value: <Badge variant={state.variant}>{state.label}</Badge>,
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
      value: formatTimeAgo(peer.latestPositiveObservedAt * 1000),
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
      {detail.isLoading ? (
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
 * the retained evidence.
 */
function PeerRow({ peer, folded }: { peer: NetworkPeerSummary; folded: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const columns = peerColumns(peer);
  const state = crawlerDialStateBadge(peer.crawlerDialState);
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
          <button
            type="button"
            {...toggle}
            className="grid min-h-[44px] w-full grid-cols-[14px_auto_minmax(0,1fr)] items-start gap-x-2 gap-y-0.5 px-1 py-2 text-left font-mono sm:px-3"
          >
            <span aria-hidden="true" className="text-text-dim text-small">
              {expanded ? '▾' : '▸'}
            </span>
            {/* The id keeps its width (already cut in the middle); the dial state takes what is
                left of the line and wraps inside its badge only when it must. */}
            <span className="text-aqua text-small whitespace-nowrap">
              {truncateHash(peer.peerId)}
            </span>
            <span className="min-w-0 justify-self-end">
              <Badge variant={state.variant} className="text-meta leading-snug">
                {state.label}
              </Badge>
            </span>
            <span className="text-text text-meta col-span-2 col-start-2 min-w-0 truncate">
              {orDash(peer.primaryAddr)}
            </span>
          </button>
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
            <span>{truncateHash(peer.peerId)}</span>
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

  const { data, isLoading, isError, isFetching } = useQuery({
    queryKey: ['network', 'peers', filters, cursor],
    queryFn: () => api.getNetworkPeers({ ...filters, cursor }),
  });

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

  const showSkeleton = isLoading && rows.length === 0;
  const showEmpty = !isLoading && !isError && rows.length === 0;
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
          className="border-base-border bg-base-surface text-text hover:text-text-bright text-small h-8 rounded-md border px-2.5 font-mono md:hidden"
        >
          filters{activeFilters > 0 ? ` · ${activeFilters}` : ''}{' '}
          <span aria-hidden="true" className="text-text-dim">
            {filtersOpen ? '▴' : '▾'}
          </span>
        </button>
      }
    >
      <div aria-busy={isFetching} ref={boxRef} className="border-base-border-subtle border-y">
        <div>
          <div
            id={filtersId}
            className={cn(
              'border-base-border flex-wrap items-center gap-3 border-b px-4 py-2',
              filtersOpen ? 'flex' : 'hidden md:flex'
            )}
          >
            <select
              value={filters.state ?? ''}
              onChange={(event) =>
                applyFilters({
                  ...filters,
                  state: (event.target.value || undefined) as NetworkPeerDisplayState | undefined,
                })
              }
              aria-label="Filter by crawler dial state"
              className="border-base-border bg-base-bg text-text text-small min-w-0 max-w-full rounded border px-2 py-0.5 font-mono"
            >
              <option value="">All crawler dial states</option>
              {STATE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <select
              value={filters.observation ?? ''}
              onChange={(event) =>
                applyFilters({
                  ...filters,
                  observation: (event.target.value || undefined) as
                    | NetworkAddressProbeResult
                    | undefined,
                })
              }
              aria-label="Filter by address observation"
              className="border-base-border bg-base-bg text-text text-small min-w-0 max-w-full rounded border px-2 py-0.5 font-mono"
            >
              <option value="">All address observations</option>
              {OBSERVATION_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <input
              type="text"
              value={filters.country ?? ''}
              onChange={(event) =>
                applyFilters({ ...filters, country: event.target.value || undefined })
              }
              placeholder="Country"
              aria-label="Filter by country"
              className="border-base-border bg-base-bg text-text placeholder:text-text-dim text-small min-w-0 max-w-full rounded border px-2 py-0.5 font-mono"
            />
            <input
              type="text"
              value={filters.version ?? ''}
              onChange={(event) =>
                applyFilters({ ...filters, version: event.target.value || undefined })
              }
              placeholder="Version"
              aria-label="Filter by version"
              className="border-base-border bg-base-bg text-text placeholder:text-text-dim text-small min-w-0 max-w-full rounded border px-2 py-0.5 font-mono"
            />
          </div>

          <div>
            {!folded && (
              <div
                className="border-base-border bg-base-surface/50 text-text-dim text-small grid items-center gap-x-4 border-b px-4 py-2 font-mono"
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
                No peers match these evidence filters
              </div>
            ) : (
              rows.map((peer) => <PeerRow key={peer.peerId} peer={peer} folded={folded} />)
            )}
          </div>
        </div>

        {data?.nextCursor != null && (
          <div className="border-base-border-subtle flex justify-center border-t px-4 py-2">
            <button
              type="button"
              onClick={() => setCursor(data.nextCursor ?? undefined)}
              disabled={isFetching}
              className="text-text-dim hover:text-interactive text-small font-mono transition-colors disabled:opacity-50"
            >
              {isFetching ? 'Loading…' : 'Load more'}
            </button>
          </div>
        )}
      </div>
    </DetailSection>
  );
}
