'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { StatBlock, StatGrid } from '@/components/ui/stat-block';
import { api, NetworkLastRound } from '@/lib/api';
import { formatTimeAgo } from '@/lib/utils';
import { NetworkDistributions } from '@/app/network/distributions';
import { NetworkTrends } from '@/app/network/trends';

function PageShell({ children }: { children: ReactNode }) {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader
          title="Peers"
          subtitle="Whole-network CKB L1 peer discovery — the nodes this crawler can reach"
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
        Honest caveat: these are <strong>discoverable / reachable nodes only</strong> — not the full
        network. Nodes behind NAT or firewalls, or that refuse dials, stay hidden.
      </p>
    </div>
  );
}

function PeersOnboarding({ enabled }: { enabled: boolean }) {
  return (
    <div className="space-y-4">
      {enabled && (
        <div className="border-info/30 bg-info/10 rounded border px-4 py-3">
          <p className="text-info font-mono text-sm">
            Crawler enabled — waiting for the first round to finish. Peer data appears here once the
            first crawl completes (this can take a few minutes).
          </p>
        </div>
      )}

      <div className="border-base-border bg-base-surface rounded border p-6">
        <h2 className="text-text-bright mb-2 font-mono text-lg font-bold">The CKB peer crawler</h2>
        <p className="text-text-dim mb-4 text-sm leading-relaxed">
          The peer crawler performs whole-network CKB L1 node discovery: starting from bootnodes it
          dials outward across the p2p network and records every node it can reach. It is{' '}
          <span className="text-text">local-first</span> — you run the crawl, and the resulting
          dataset is yours.
        </p>

        <ReachabilityCaveat className="mb-4" />

        <h3 className="text-text-bright mb-2 font-mono text-sm font-bold">How to enable</h3>
        <p className="text-text-dim mb-2 text-sm">
          Set the crawler to enabled in <code className="text-aqua">ckbadger.toml</code> and
          restart:
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

function PeersDashboard({ lastRound }: { lastRound: NetworkLastRound }) {
  const updatedAgo = formatTimeAgo(lastRound.finished * 1000);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-text-dim font-mono text-xs">
          Round #{lastRound.roundId} · updated {updatedAgo}
        </span>
        {!lastRound.frontierDrained && (
          <Badge variant="gold" className="uppercase">
            partial round
          </Badge>
        )}
      </div>

      <StatGrid columns={4}>
        <StatBlock label="Discovered Reachable" value={lastRound.reachable} color="jade" />
        <StatBlock label="Unreachable" value={lastRound.unreachable} color="rouge" />
        <StatBlock label="Total Known" value={lastRound.totalKnown} color="aqua" />
        <StatBlock label="Last Round" value={updatedAgo} color="gold" />
      </StatGrid>

      <ReachabilityCaveat />

      <NetworkDistributions />
      <NetworkTrends />
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
    content = <PeersOnboarding enabled={summary.enabled} />;
  } else {
    content = <PeersDashboard lastRound={summary.lastRound} />;
  }

  return <PageShell>{content}</PageShell>;
}
