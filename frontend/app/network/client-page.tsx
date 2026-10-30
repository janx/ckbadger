'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { Badge } from '@/components/ui/page-header';
import { api, LabelCount, NetworkActiveRound, NetworkLastRound } from '@/lib/api';
import { formatTimeAgo, formatUtc8DateTime } from '@/lib/utils';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { HostingBar, NetworkDistributions, NetworkProtocols } from '@/app/network/distributions';
import { NetworkShareHistory, NetworkTrends } from '@/app/network/trends';
import { PeersTable } from '@/app/network/nodes-table';
import { AdvertisementGraph } from '@/app/network/advertisement-graph';
import { NetworkMapView } from '@/app/network/network-map';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';

function PageShell({ children }: { children: ReactNode }) {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">{children}</main>
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="space-y-4">
      <div className="bg-base-elevated h-40 w-full animate-pulse rounded" />
      <div className="grid grid-cols-1 gap-4 min-[480px]:grid-cols-2 md:grid-cols-4">
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="bg-base-elevated h-24 animate-pulse rounded" />
        ))}
      </div>
    </div>
  );
}

function ReachabilityCaveat({ className }: { className?: string }) {
  return (
    <div
      className={`border-base-border bg-base-surface rounded border px-4 py-3 ${className ?? ''}`}
    >
      <p className="text-text-dim font-mono text-xs leading-relaxed">
        Evidence boundary: an advertised candidate is not verified until this crawler completes an
        authenticated, same-network Identify exchange. These local observations are not the full
        network; nodes behind NAT or firewalls can stay hidden.
      </p>
    </div>
  );
}

function PeersOnboarding({
  enabled,
  activeRound,
}: {
  enabled: boolean;
  activeRound: NetworkActiveRound | null;
}) {
  return (
    <div className="space-y-4">
      {enabled && (
        <div className="border-info/30 bg-info/10 rounded border px-4 py-3">
          <p className="text-info font-mono text-sm">
            {activeRound?.blockedReason
              ? `Crawler round #${activeRound.roundId} blocked — ${activeRound.blockedReason}`
              : activeRound
                ? `Crawler round #${activeRound.roundId} in progress — ${activeRound.completedPeers}/${activeRound.candidatePeers} peer candidates completed, ${activeRound.addressAttempts} address attempts.`
                : 'Crawler enabled — waiting for the first round to start. Peer data appears here once the first crawl completes.'}
          </p>
        </div>
      )}

      <div className="border-base-border bg-base-surface rounded border p-6">
        <h2 className="text-text-bright mb-2 font-mono text-lg font-bold">The CKB peer crawler</h2>
        <p className="text-text-dim mb-4 text-sm leading-relaxed">
          The peer crawler performs whole-network CKB L1 node discovery: starting from bootnodes it
          dials outward across the p2p network and records each same-network peer this observer
          verifies. It is <span className="text-text">local-first</span> — you run the crawl, and
          the resulting dataset is yours.
        </p>

        <ReachabilityCaveat className="mb-4" />

        <h3 className="text-text-bright mb-2 font-mono text-sm font-bold">How to enable</h3>
        <p className="text-text-dim mb-2 text-sm">
          Set the crawler to enabled in{' '}
          <code className="text-aqua">this network&apos;s config.toml</code> and restart:
        </p>
        <pre className="border-base-border bg-base-bg text-text overflow-x-auto rounded border p-3 font-mono text-xs">
          {`[crawler]
enabled = true
# optional geo / ASN enrichment (MaxMind GeoLite2):
# geoip_city_path = "/path/to/GeoLite2-City.mmdb"
# geoip_asn_path  = "/path/to/GeoLite2-ASN.mmdb"`}
        </pre>
        <p className="text-text-dim mt-3 text-xs leading-relaxed">
          Note: enabling performs outbound whole-network crawling from your host. Geo / ASN columns
          stay empty unless the optional MaxMind GeoLite2 databases are configured.
        </p>
      </div>
    </div>
  );
}

// Geo/ASN is only present when the optional MaxMind GeoLite2 databases are configured. When they
// are not, every country groups under "Unknown" — so real geo means at least one non-"Unknown"
// country label. Only then do we owe MaxMind an attribution.
function hasGeoData(countries: LabelCount[]): boolean {
  return countries.some((c) => {
    const label = c.label.trim();
    return label.length > 0 && label !== 'Unknown';
  });
}

// Shares the ['network', 'distributions'] query key with <NetworkDistributions/>, so this is a
// single fetch (react-query dedupes) rather than a second request or a divergent geo signal.
function MaxMindAttribution() {
  const { data } = useQuery({
    queryKey: ['network', 'distributions'],
    queryFn: api.getNetworkDistributions,
    refetchInterval: 30000,
  });

  if (!data || !hasGeoData(data.countries)) return null;

  return (
    <p className="text-text-dim border-base-border/60 mt-2 border-t pt-3 font-mono text-xs">
      Geo/ASN data © MaxMind (GeoLite2)
    </p>
  );
}

/**
 * The latest round in four quiet figures, the board's wording: no tracked capitals and no alarm
 * red, because "not reachable now" is not "gone" (CH-53).
 */
function RoundFigures({ lastRound }: { lastRound: NetworkLastRound }) {
  const network = useActiveNetwork();
  const minutes = Math.round((lastRound.finishedAt - lastRound.startedAt) / 60);
  if (!(minutes >= 0)) throw new Error(`crawler round #${lastRound.roundId} ends before it starts`);
  const figure = (label: string, value: string, note?: string, jade = false) => (
    <div key={label}>
      <dt className="text-text-dim text-[12px]">{label}</dt>
      <dd className={`mt-0.5 text-[18px] tabular-nums ${jade ? 'text-jade' : 'text-text-bright'}`}>
        {value}
      </dd>
      {note && <dd className="text-text-dim mt-0.5 text-[11.5px]">{note}</dd>}
    </div>
  );
  return (
    <dl role="group" aria-label="Latest crawler round" className="grid grid-cols-2 gap-x-7 gap-y-4">
      {figure(
        'Reachable now',
        lastRound.reachablePeers.toLocaleString(),
        `answered as CKB ${network} nodes`,
        true
      )}
      {figure('Verified before, not now', lastRound.verifiedUnavailablePeers.toLocaleString())}
      {figure(
        'Heard of',
        lastRound.candidatePeers.toLocaleString(),
        `${lastRound.peerOutcomes.exhausted.withoutRetainedVerification.toLocaleString()} never verified`
      )}
      {figure(
        'Latest round',
        `#${lastRound.roundId}`,
        `${formatUtc8DateTime(lastRound.finishedAt * 1000).slice(5, 16)} UTC+8 · ${minutes} min · ${lastRound.addressAttempts.toLocaleString()} dials`
      )}
    </dl>
  );
}

function PeersDashboard({
  lastRound,
  activeRound,
  enabled,
}: {
  lastRound: NetworkLastRound;
  activeRound: NetworkActiveRound | null;
  enabled: boolean;
}) {
  const updatedAgo = formatTimeAgo(lastRound.finishedAt * 1000);

  return (
    <DetailLayout
      title="Where the nodes are"
      kind="topic"
      identity={
        <div className="grid min-w-0 items-end gap-x-12 gap-y-6 lg:grid-cols-[minmax(0,1fr)_460px]">
          <p className="text-text max-w-[720px] text-[15px] leading-relaxed">
            CKB nodes find each other by word of mouth: each node tells its peers about other nodes
            it knows. This crawler asks every node it can reach, then tries to connect to each one
            it hears about. A node it cannot reach may still be running, behind NAT or a firewall.
          </p>
          <RoundFigures lastRound={lastRound} />
        </div>
      }
    >
      {!enabled && (
        <p role="status" className="text-info text-[12px]">
          Crawler disabled. This retained evidence is not live.
        </p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-text-dim text-xs">
          Round #{lastRound.roundId} · updated {updatedAgo}
        </span>
        {activeRound && (
          <Badge variant="blue">
            {activeRound.blockedReason
              ? `round #${activeRound.roundId} blocked`
              : `round #${activeRound.roundId} crawling ${activeRound.completedPeers}/${activeRound.candidatePeers}`}
          </Badge>
        )}
      </div>

      {activeRound?.blockedReason && (
        <p className="border-negative/30 bg-negative/10 text-negative rounded border px-3 py-2 font-mono text-xs">
          Active crawler round blocked: {activeRound.blockedReason}
        </p>
      )}

      <DetailSection title="Verified peers across the world">
        <NetworkMapView
          neverVerified={lastRound.peerOutcomes.exhausted.withoutRetainedVerification}
        />
        <div className="mt-6">
          <HostingBar />
        </div>
      </DetailSection>
      <NetworkDistributions />
      <NetworkTrends />
      <MaxMindAttribution />
      <DetailTier2>
        <DetailSection title="Crawler round evidence">
          <p className="text-text-dim text-xs leading-relaxed">
            Round #{lastRound.roundId}: {lastRound.addressAttempts.toLocaleString()} address
            attempts · {lastRound.nonSuccessfulAddressAttempts.toLocaleString()} non-successful
            observations · {lastRound.exhaustedCandidates.toLocaleString()} exhausted candidates ·{' '}
            {lastRound.foreignPeers.toLocaleString()} foreign peers ·{' '}
            {lastRound.newVerifiedPeers.toLocaleString()} newly verified
          </p>
          <ReachabilityCaveat className="mt-3" />
        </DetailSection>
        <DetailSection title="Protocols they speak">
          <NetworkProtocols />
        </DetailSection>
        <DetailSection title="Daily version and country counts">
          <NetworkShareHistory />
        </DetailSection>
        <DetailSection title="Who named whom">
          <AdvertisementGraph roundId={lastRound.roundId} />
        </DetailSection>
        <DetailSection title="Peer evidence">
          <PeersTable />
        </DetailSection>
      </DetailTier2>
    </DetailLayout>
  );
}

export function NetworkClientPage() {
  const {
    data: summary,
    isLoading,
    error,
  } = useQuery({
    queryKey: ['network', 'summary'],
    queryFn: api.getNetworkSummary,
    refetchInterval: 30000,
  });

  let content: ReactNode;
  if (error) {
    content = (
      <DetailLayout title="Where the nodes are" kind="topic">
        <p role="alert" className="text-negative">
          Crawler summary unavailable: {String(error)}
        </p>
      </DetailLayout>
    );
  } else if (isLoading || !summary) {
    content = <LoadingSkeleton />;
  } else if (!summary.hasData || !summary.lastRound) {
    content = (
      <DetailLayout
        title="Where the nodes are"
        kind="topic"
        identity="As this crawler saw the CKB L1 network."
      >
        <PeersOnboarding enabled={summary.enabled} activeRound={summary.activeRound} />
      </DetailLayout>
    );
  } else {
    content = (
      <PeersDashboard
        lastRound={summary.lastRound}
        activeRound={summary.activeRound}
        enabled={summary.enabled}
      />
    );
  }

  return <PageShell>{content}</PageShell>;
}
