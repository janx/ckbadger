'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';
import { api, LabelCount, NetworkActiveRound, NetworkLastRound } from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';
import { NetworkDistributions } from '@/app/network/distributions';
import { NetworkTrends } from '@/app/network/trends';
import { PeersTable } from '@/app/network/nodes-table';

function PageShell({ children }: { children: ReactNode }) {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader
          title="Peers"
          subtitle="CKB L1 peer advertisements and same-network reachability evidence from this crawler"
        />
        {children}
      </main>
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
    <div className={`border-warning/30 bg-warning/10 rounded border px-4 py-3 ${className ?? ''}`}>
      <p className="text-warning font-mono text-xs leading-relaxed">
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

function PeersDashboard({
  lastRound,
  activeRound,
}: {
  lastRound: NetworkLastRound;
  activeRound: NetworkActiveRound | null;
}) {
  const updatedAgo = formatTimeAgo(lastRound.finishedAt * 1000);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-text-dim font-mono text-xs">
          Round #{lastRound.roundId} · updated {updatedAgo}
        </span>
        {activeRound && (
          <Badge variant="gold" className="uppercase">
            {activeRound.blockedReason
              ? `round #${activeRound.roundId} blocked`
              : `round #${activeRound.roundId} crawling ${activeRound.completedPeers}/${activeRound.candidatePeers}`}
          </Badge>
        )}
      </div>

      <StatGrid columns={4}>
        <StatBlock label="Advertised candidates" value={lastRound.candidatePeers} color="gold" />
        <StatBlock label="Same-network reachable" value={lastRound.reachablePeers} color="jade" />
        <StatBlock label="Verified retained" value={lastRound.verifiedRetainedPeers} color="aqua" />
        <StatBlock
          label="Verified unavailable"
          value={lastRound.verifiedUnavailablePeers}
          color="rouge"
        />
      </StatGrid>

      <p className="text-text-dim font-mono text-xs">
        Last round: {lastRound.addressAttempts} address attempts ·{' '}
        {lastRound.nonSuccessfulAddressAttempts} non-successful observations ·{' '}
        {lastRound.exhaustedCandidates} exhausted candidates · {lastRound.foreignPeers} foreign
        peers · {lastRound.newVerifiedPeers} newly verified
      </p>

      {activeRound?.blockedReason && (
        <p className="border-negative/30 bg-negative/10 text-negative rounded border px-3 py-2 font-mono text-xs">
          Active crawler round blocked: {activeRound.blockedReason}
        </p>
      )}

      <ReachabilityCaveat />

      <NetworkDistributions />
      <NetworkTrends />
      <PeersTable />
      <MaxMindAttribution />
    </div>
  );
}

export function NetworkClientPage() {
  const { data: summary, isLoading } = useQuery({
    queryKey: ['network', 'summary'],
    queryFn: api.getNetworkSummary,
    refetchInterval: 30000,
  });

  let content: ReactNode;
  if (isLoading || !summary) {
    content = <LoadingSkeleton />;
  } else if (!summary.enabled || !summary.hasData || !summary.lastRound) {
    content = <PeersOnboarding enabled={summary.enabled} activeRound={summary.activeRound} />;
  } else {
    content = <PeersDashboard lastRound={summary.lastRound} activeRound={summary.activeRound} />;
  }

  return <PageShell>{content}</PageShell>;
}
