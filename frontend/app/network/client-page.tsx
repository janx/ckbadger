'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { api, LabelCount, NetworkActiveRound, NetworkLastRound } from '@/lib/api';
import { formatTimeAgo, formatUtc8DateTime } from '@/lib/utils';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { HostingBar, NetworkDistributions, NetworkProtocols } from '@/app/network/distributions';
import { NetworkShareHistory, NetworkTrends } from '@/app/network/trends';
import { PeersTable } from '@/app/network/nodes-table';
import { AdvertisementGraph } from '@/app/network/advertisement-graph';
import { NetworkMapFootnotes, NetworkMapPicture, useNetworkMap } from '@/app/network/network-map';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';

const PAGE_TITLE = 'Where the nodes are';

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
      <p className="text-text-dim text-small font-mono leading-relaxed">
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
          <p className="text-info text-body font-mono">
            {activeRound?.blockedReason
              ? `Crawler round #${activeRound.roundId} blocked — ${activeRound.blockedReason}`
              : activeRound
                ? `Crawler round #${activeRound.roundId} in progress — ${activeRound.completedPeers}/${activeRound.candidatePeers} peer candidates completed, ${activeRound.addressAttempts} address attempts.`
                : 'Crawler enabled — waiting for the first round to start. Peer data appears here once the first crawl completes.'}
          </p>
        </div>
      )}

      <div className="border-base-border bg-base-surface rounded border p-6">
        <h2 className="text-text-bright text-section mb-2 font-mono font-bold">
          The CKB peer crawler
        </h2>
        <p className="text-text-dim text-body mb-4 leading-relaxed">
          The peer crawler performs whole-network CKB L1 node discovery: starting from bootnodes it
          dials outward across the p2p network and records each same-network peer this observer
          verifies. It is <span className="text-text">local-first</span> — you run the crawl, and
          the resulting dataset is yours.
        </p>

        <ReachabilityCaveat className="mb-4" />

        <h3 className="text-text-bright text-body mb-2 font-mono font-bold">How to enable</h3>
        <p className="text-text-dim text-body mb-2">
          Set the crawler to enabled in{' '}
          <code className="text-aqua">this network&apos;s config.toml</code> and restart:
        </p>
        <pre className="border-base-border bg-base-bg text-text text-small overflow-x-auto rounded border p-3 font-mono">
          {`[crawler]
enabled = true
# optional geo / ASN enrichment (MaxMind GeoLite2):
# geoip_city_path = "/path/to/GeoLite2-City.mmdb"
# geoip_asn_path  = "/path/to/GeoLite2-ASN.mmdb"`}
        </pre>
        <p className="text-text-dim text-small mt-3 leading-relaxed">
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
    <p className="text-text-dim border-base-border/60 text-small mt-2 border-t pt-3 font-mono">
      Geo/ASN data © MaxMind (GeoLite2)
    </p>
  );
}

/** One figure of the hero's quiet row (R13): a meta label over a section-size value, its note inline. */
function Metric({ label, children, note }: { label: string; children: ReactNode; note?: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-meta">{label}</dt>
      <dd className="text-text-bright text-section mt-0.5 tabular-nums">
        {children}
        {note && <span className="text-text-dim text-small"> · {note}</span>}
      </dd>
    </div>
  );
}

/**
 * The latest round's other figures in one quiet row, the board's wording: no tracked capitals and
 * no alarm red, because "not reachable now" is not "gone" (CH-53). Reachable now is the hero's figure.
 */
function RoundFigures({ lastRound }: { lastRound: NetworkLastRound }) {
  const minutes = Math.round((lastRound.finishedAt - lastRound.startedAt) / 60);
  if (!(minutes >= 0)) throw new Error(`crawler round #${lastRound.roundId} ends before it starts`);
  return (
    <dl
      role="group"
      aria-label="Latest crawler round"
      className="mt-7 grid grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-3"
    >
      <Metric label="Verified before, not now">
        {lastRound.verifiedUnavailablePeers.toLocaleString()}
      </Metric>
      <Metric
        label="Heard of"
        note={`${lastRound.peerOutcomes.exhausted.withoutRetainedVerification.toLocaleString()} never verified`}
      >
        {lastRound.candidatePeers.toLocaleString()}
      </Metric>
      <Metric
        label="Latest round"
        note={`${formatUtc8DateTime(lastRound.finishedAt * 1000).slice(5, 16)} UTC+8 · ${minutes} min · ${lastRound.addressAttempts.toLocaleString()} dials`}
      >
        #{lastRound.roundId}
      </Metric>
    </dl>
  );
}

/** The page title (index step), in the hero's title row. */
function NetworkHeading() {
  return <h1 className="text-text-bright text-index font-medium">{PAGE_TITLE}</h1>;
}

/** R13: one short sentence (≤ 90 characters) and the way down to the explanation in tier 2. */
function NetworkLede() {
  return (
    <p data-role="lede" className="text-text-dim text-body mt-1.5 max-w-[560px]">
      Nodes find each other by word of mouth; unreachable is not gone.{' '}
      <a href="#evidence" className="hover:text-text-bright whitespace-nowrap hover:underline">
        How it works ↓
      </a>
    </p>
  );
}

/**
 * The hero (hierarchy v3, R13): the title row (h1, the round and its age, a round in progress as
 * plain words), one sentence, reachable now as the page's one figure in jade (R11) with its
 * companion on the same baseline, and the map right under it. The map's lines and the round's other
 * figures in one quiet row follow the section.
 */
function Hero({
  lastRound,
  activeRound,
  enabled,
}: {
  lastRound: NetworkLastRound;
  activeRound: NetworkActiveRound | null;
  enabled: boolean;
}) {
  const network = useActiveNetwork();
  const map = useNetworkMap(lastRound.peerOutcomes.exhausted.withoutRetainedVerification);
  const updatedAgo = formatTimeAgo(lastRound.finishedAt * 1000);
  return (
    <div className="min-w-0">
      <section aria-label={PAGE_TITLE} className="min-w-0">
        <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
          <NetworkHeading />
          <p className="text-text-dim text-small">
            Round #{lastRound.roundId} · updated {updatedAgo}
            {activeRound &&
              (activeRound.blockedReason
                ? ` · round #${activeRound.roundId} blocked`
                : ` · round #${activeRound.roundId} crawling ${activeRound.completedPeers}/${activeRound.candidatePeers}`)}
          </p>
        </div>
        <NetworkLede />
        {!enabled && (
          <p role="status" className="text-info text-small mt-3">
            Crawler disabled. This retained evidence is not live.
          </p>
        )}
        {activeRound?.blockedReason && (
          <p className="border-negative/30 bg-negative/10 text-negative text-small mt-3 rounded border px-3 py-2 font-mono">
            Active crawler round blocked: {activeRound.blockedReason}
          </p>
        )}
        <div data-role="figure" className="mt-9 min-w-0">
          <div className="text-text-dim text-meta mb-1.5">Reachable now</div>
          <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
            <span
              data-role="reachable"
              className="text-jade text-index sm:text-display font-medium tabular-nums"
            >
              {lastRound.reachablePeers.toLocaleString()}
            </span>
            <span className="text-text-dim text-body">answered as CKB {network} nodes</span>
          </div>
        </div>
        <div className="mt-[18px] min-w-0">
          <NetworkMapPicture map={map} />
        </div>
      </section>
      <NetworkMapFootnotes map={map} />
      <RoundFigures lastRound={lastRound} />
    </div>
  );
}

/** Tier 2's first section: what the hero states as a figure and a map, in words. */
function HowTheCrawlerSees() {
  return (
    <DetailSection title="How the crawler sees the network">
      <p className="text-text-dim text-body">
        CKB nodes find each other by word of mouth: each node tells its peers about other nodes it
        knows. This crawler asks every node it can reach, then tries to connect to each one it hears
        about. A node it cannot reach may still be running, behind NAT or a firewall.
      </p>
    </DetailSection>
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
  return (
    <div className="text-body min-w-0 font-mono">
      <div data-tier="1" className="min-w-0 space-y-8">
        <Hero lastRound={lastRound} activeRound={activeRound} enabled={enabled} />
        <HostingBar />
        <NetworkDistributions />
        <NetworkTrends />
        <MaxMindAttribution />
      </div>
      {/* "How it works ↓" in the hero lands on the evidence boundary. */}
      <div id="evidence" className="scroll-mt-4">
        <DetailTier2>
          <HowTheCrawlerSees />
          <DetailSection title="Crawler round evidence">
            <p className="text-text-dim text-small leading-relaxed">
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
          <PeersTable />
        </DetailTier2>
      </div>
    </div>
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
      <DetailLayout title={PAGE_TITLE} kind="topic">
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
        title={PAGE_TITLE}
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
