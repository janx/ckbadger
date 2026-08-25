'use client';

import { Fragment, useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalPanelHeader,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
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
import { formatTimeAgo, truncateHash } from '@/lib/utils';

interface PeerFilters {
  state?: NetworkPeerDisplayState;
  observation?: NetworkAddressProbeResult;
  country?: string;
  version?: string;
}

const GRID_TEMPLATE = '13rem 15rem 18rem 14rem 17rem 7rem 9rem 9rem 8rem 8rem 5rem';
const GRID_MIN_WIDTH = '132rem';
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
  variant: 'green' | 'gold' | 'blue' | 'red' | 'gray';
} {
  switch (state) {
    case 'reachable':
      return { label: 'Same-network Identify', variant: 'green' };
    case 'verifiedUnavailable':
      return { label: 'Aliases exhausted · prior verification', variant: 'gold' };
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
    <div className="border-base-border bg-base-bg/70 space-y-4 border-t px-4 py-4 font-mono text-xs">
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

function PeerRow({ peer }: { peer: NetworkPeerSummary }) {
  const [expanded, setExpanded] = useState(false);
  const detail = useQuery({
    queryKey: ['network', 'peer', peer.peerId],
    queryFn: () => api.getNetworkPeer(peer.peerId),
    enabled: expanded,
  });
  const state = crawlerDialStateBadge(peer.crawlerDialState);
  const participation = [
    peer.participation.discoveryAdvertised ? 'Discovery ad' : null,
    peer.participation.directSessionObserved ? 'Direct CKB session' : null,
    peer.participation.crawlerIdentified ? 'Identify' : null,
  ].filter((value): value is string => value != null);

  return (
    <Fragment>
      <TerminalRow data-testid="peer-row">
        <div
          className="grid w-full items-center gap-x-4"
          style={{ gridTemplateColumns: GRID_TEMPLATE, minWidth: GRID_MIN_WIDTH }}
        >
          <button
            type="button"
            onClick={() => setExpanded((value) => !value)}
            className="text-aqua truncate text-left font-mono text-xs hover:underline"
            title={peer.peerId}
            aria-expanded={expanded}
            aria-label={`${expanded ? 'Hide' : 'Show'} evidence for ${peer.peerId}`}
          >
            {expanded ? '▾ ' : '▸ '}
            <span>{truncateHash(peer.peerId)}</span>
          </button>
          <span
            className="text-text truncate font-mono text-xs"
            title={peer.primaryAddr ?? undefined}
          >
            {orDash(peer.primaryAddr)}
          </span>
          <span className="text-text-dim truncate text-xs" title={participation.join(' · ')}>
            {participation.join(' · ') || '—'}
          </span>
          <span className="text-text-dim truncate font-mono text-xs">
            {peer.sessionInitiators.map(sessionInitiatorLabel).join(' · ') || '—'}
          </span>
          <span>
            <Badge variant={state.variant}>{state.label}</Badge>
          </span>
          <span className="text-text truncate font-mono text-xs">{orDash(peer.version)}</span>
          <span className="text-text truncate text-xs" title={peer.country ?? undefined}>
            {orDash(peer.country)}
          </span>
          <span className="text-text-dim text-xs">
            {formatTimeAgo(peer.latestPositiveObservedAt * 1000)}
          </span>
          <span className="text-text-dim text-xs">{optionalAge(peer.lastDialObservedAt)}</span>
          <span className="text-text-dim text-xs">{optionalAge(peer.lastReachableAt)}</span>
          <span className="text-text-dim text-right font-mono text-xs tabular-nums">
            {peer.rttMs == null ? '—' : `${peer.rttMs} ms`}
          </span>
        </div>
      </TerminalRow>
      {expanded && (
        <TerminalRow data-testid="peer-evidence-row">
          {detail.isLoading ? (
            <div className="text-text-dim px-4 py-3 font-mono text-xs">Loading evidence…</div>
          ) : detail.isError || !detail.data ? (
            <div className="text-negative px-4 py-3 font-mono text-xs">
              Failed to load peer evidence.
            </div>
          ) : (
            <EvidenceDetail detail={detail.data} />
          )}
        </TerminalRow>
      )}
    </Fragment>
  );
}

export function PeersTable() {
  const [filters, setFilters] = useState<PeerFilters>({});
  const [cursor, setCursor] = useState<string | undefined>();
  const [rows, setRows] = useState<NetworkPeerSummary[]>([]);
  const mergedTokenRef = useRef<string | null>(null);

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

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright font-mono text-lg font-bold">Peer evidence</h2>

      <TerminalPanel>
        <TerminalPanelHeader indicator={isFetching ? 'active' : 'none'}>
          Observed network peers
        </TerminalPanelHeader>
        <TerminalPanelContent padding="none">
          <div className="border-base-border flex flex-wrap items-center gap-3 border-b px-4 py-2">
            <select
              value={filters.state ?? ''}
              onChange={(event) =>
                applyFilters({
                  ...filters,
                  state: (event.target.value || undefined) as NetworkPeerDisplayState | undefined,
                })
              }
              aria-label="Filter by crawler dial state"
              className="border-base-border bg-base-bg text-text rounded border px-2 py-0.5 font-mono text-xs"
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
              className="border-base-border bg-base-bg text-text rounded border px-2 py-0.5 font-mono text-xs"
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
              className="border-base-border bg-base-bg text-text placeholder:text-text-dim rounded border px-2 py-0.5 font-mono text-xs"
            />
            <input
              type="text"
              value={filters.version ?? ''}
              onChange={(event) =>
                applyFilters({ ...filters, version: event.target.value || undefined })
              }
              placeholder="Version"
              aria-label="Filter by version"
              className="border-base-border bg-base-bg text-text placeholder:text-text-dim rounded border px-2 py-0.5 font-mono text-xs"
            />
          </div>

          <div className="overflow-x-auto">
            <div
              className="border-base-border bg-base-surface/50 text-text-dim grid items-center gap-x-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider"
              style={{ gridTemplateColumns: GRID_TEMPLATE, minWidth: GRID_MIN_WIDTH }}
            >
              {COLUMNS.map((label, index) => (
                <div key={label} className={index === COLUMNS.length - 1 ? 'text-right' : ''}>
                  {label}
                </div>
              ))}
            </div>

            {showSkeleton ? (
              <div className="text-text-dim py-12 text-center font-mono text-sm">
                Loading peer evidence…
              </div>
            ) : isError ? (
              <div className="text-negative py-12 text-center font-mono text-sm">
                Failed to load peer evidence.
              </div>
            ) : showEmpty ? (
              <div className="text-text-dim py-12 text-center font-mono text-sm">
                No peers match these evidence filters
              </div>
            ) : (
              rows.map((peer) => <PeerRow key={peer.peerId} peer={peer} />)
            )}
          </div>
        </TerminalPanelContent>

        {data?.nextCursor != null && (
          <TerminalPanelFooter className="flex justify-center">
            <button
              type="button"
              onClick={() => setCursor(data.nextCursor ?? undefined)}
              disabled={isFetching}
              className="text-text-dim hover:text-interactive font-mono text-xs transition-colors disabled:opacity-50"
            >
              {isFetching ? 'Loading…' : 'Load more'}
            </button>
          </TerminalPanelFooter>
        )}
      </TerminalPanel>
    </section>
  );
}
