'use client';

import { useQuery } from '@tanstack/react-query';
import { api, LabelCount } from '@/lib/api';
import { DATA_COLOR } from '@/lib/design-tokens';
import { fade } from '@/lib/palette';
import { formatPercent } from '@/lib/ckb-decimal';
import { partitionSpans } from '@/lib/partition-geometry';
import { useElementWidth } from '@/hooks/useElementWidth';
import { cn } from '@/lib/utils';
import { TopicSection } from '@/app/network/topic-section';

const TOP_N = 8;
/** The hosting bar's segments: one hue, the largest strongest (Network board). */
const PROVIDER_OPACITY = [0.9, 0.6, 0.5, 0.42, 0.34, 0.2] as const;
/** The in-segment words (hierarchy v3 R15): 12 px monospace, 10 px padding each side. */
const LABEL_ADVANCE = 7.2;
const LABEL_PAD = 10;

/** The data hue at `alpha`, as a background, so a segment's words keep their full ink. */
function dataHue(alpha: number): string {
  return fade(DATA_COLOR, alpha);
}

function useDistributions() {
  // Shares the ['network', 'distributions'] key with the MaxMind attribution: one fetch.
  return useQuery({
    queryKey: ['network', 'distributions'],
    queryFn: api.getNetworkDistributions,
    refetchInterval: 30000,
  });
}

/**
 * A top-N bar list in the data hue, the leader strongest (Network board). Labels stay DOM text;
 * `attr` marks the bars for their own section.
 */
function Bars({ items, attr }: { items: LabelCount[]; attr: string }) {
  const top = [...items].sort((a, b) => b.count - a.count).slice(0, TOP_N);
  const max = top.reduce((m, it) => Math.max(m, it.count), 0);
  if (top.length === 0) return <p className="text-text-dim text-small">No data yet</p>;
  return (
    <div className="flex max-w-[720px] flex-col gap-1">
      {top.map((it, i) => (
        <div
          key={it.label}
          className="text-small grid grid-cols-[minmax(0,1fr)_90px_36px] items-center gap-2.5 py-0.5"
        >
          <span
            className={`truncate ${i === 0 ? 'text-text-bright' : 'text-text'}`}
            title={it.label}
          >
            {it.label}
          </span>
          <span aria-hidden="true" className="bg-base-elevated flex h-1.5 rounded-full">
            <span
              {...{ [attr]: '' }}
              className="rounded-full"
              style={{
                width: `${(100 * it.count) / max}%`,
                backgroundColor: DATA_COLOR,
                opacity: i === 0 ? 0.9 : 0.5,
              }}
            />
          </span>
          <span className="text-text text-right tabular-nums">{it.count.toLocaleString()}</span>
        </div>
      ))}
    </div>
  );
}

/** "Which software they run": the client versions of the verified peers (CH-54). */
export function NetworkDistributions() {
  const { data, isLoading, error } = useDistributions();
  if (isLoading) return <div className="bg-base-elevated h-40 w-full animate-pulse rounded" />;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Failed to load distributions.
      </p>
    );
  // Its scope ("reachable now or verified before") is said in tier 2 (R14: no note beside the h2).
  return (
    <TopicSection title="Which software they run">
      <Bars items={data.versions} attr="data-version-bar" />
    </TopicSection>
  );
}

/** The protocols the verified peers speak: developer evidence (tier 2, CH-54). */
export function NetworkProtocols() {
  const { data, isLoading, error } = useDistributions();
  if (isLoading) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Failed to load protocol counts.
      </p>
    );
  return <Bars items={data.protocols} attr="data-protocol-bar" />;
}

/** The organisation part of an ASN label "AS16509 Amazon.com" (the API's one label form). */
function asnOrganisation(label: string): string {
  if (label === 'Unknown') return label;
  const match = /^AS\d+ (.+)$/.exec(label);
  if (!match) throw new Error(`unexpected ASN label ${label}`);
  return match[1];
}

/**
 * "Where they run": verified nodes by hosting organisation, so one provider's several ASNs read
 * as one (Amazon's AS16509 and AS14618) — the real centralisation risk (CH-52). Hierarchy v3: a
 * 28 px bar with each organisation's name and share inside its segment where they fit (else the
 * share alone, else nothing), the exact counts in one quiet line under it, then one caption.
 */
export function HostingBar() {
  const { data, isLoading, error } = useDistributions();
  if (isLoading) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Failed to load hosting networks.
      </p>
    );
  if (data.verifiedRetained === 0) return null;
  const byOrg = new Map<string, number>();
  for (const asn of data.asns)
    byOrg.set(asnOrganisation(asn.label), (byOrg.get(asnOrganisation(asn.label)) ?? 0) + asn.count);
  const total = [...byOrg.values()].reduce((sum, count) => sum + count, 0);
  if (total !== data.verifiedRetained)
    return (
      <p role="alert" className="text-negative text-small">
        Hosting networks cover {total} peers, not the {data.verifiedRetained} verified.
      </p>
    );
  const ranked = [...byOrg.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const shown = ranked.slice(0, 5);
  const rest = ranked.slice(5).reduce((sum, [, count]) => sum + count, 0);
  const segments = rest > 0 ? [...shown, ['other networks', rest] as const] : shown;
  const leader = ranked.find(([org]) => org !== 'Unknown');
  const countries = data.countries.filter((country) => country.label !== 'Unknown').length;
  return (
    <HostingSection
      segments={segments}
      total={total}
      caption={
        leader &&
        `${leader[1].toLocaleString()} of the ${total.toLocaleString()} verified nodes run on ${leader[0]}, across ${countries} countries.`
      }
    />
  );
}

/** The section once its counts are known, so the bar measures its own width from mount. */
function HostingSection({
  segments,
  total,
  caption,
}: {
  segments: readonly (readonly [string, number])[];
  total: number;
  caption: string | undefined;
}) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const spans = partitionSpans(
    segments.map(([, count]) => BigInt(count)),
    BigInt(total),
    0,
    100,
    'hosting networks'
  );
  const share = (count: number) => `${formatPercent(BigInt(count), BigInt(total))}%`;
  /** The words a segment holds: name and share, else the share alone, else none. */
  const inside = (org: string, count: number, i: number): string => {
    if (width === null) return '';
    const room = (width * spans[i].width) / 100;
    const fits = (text: string) => text.length * LABEL_ADVANCE + 2 * LABEL_PAD <= room;
    const full = `${org} ${share(count)}`;
    return fits(full) ? full : fits(share(count)) ? share(count) : '';
  };
  return (
    <TopicSection title="Where they run">
      <div
        ref={ref}
        role="img"
        aria-label="Verified nodes by hosting network"
        title={segments.map(([org, count]) => `${org} ${share(count)}`).join(' · ')}
        className="text-small flex h-[28px] w-full gap-0.5 overflow-hidden rounded tabular-nums"
      >
        {segments.map(([org, count], i) => (
          <span
            key={org}
            data-provider={org}
            className={cn(
              'flex items-center overflow-hidden whitespace-nowrap rounded px-2.5',
              PROVIDER_OPACITY[i] >= 0.6 ? 'text-base-bg' : 'text-text-bright'
            )}
            style={{ width: `${spans[i].width}%`, backgroundColor: dataHue(PROVIDER_OPACITY[i]) }}
          >
            {inside(org, count, i) && <span>{inside(org, count, i)}</span>}
          </span>
        ))}
      </div>
      {/* R12: the exact counts leave the bar for one quiet line, which also names every segment. */}
      <p data-role="exact-amounts" className="text-text-dim text-small mt-2.5">
        {segments.map(([org, count], i) => (
          <span key={org}>
            {i > 0 && ' · '}
            {org}{' '}
            <span data-role="count" className="text-text tabular-nums">
              {count.toLocaleString()}
            </span>
          </span>
        ))}
      </p>
      {caption && (
        <p data-role="caption" className="text-text text-body mt-3.5 max-w-[640px]">
          {caption}
        </p>
      )}
    </TopicSection>
  );
}
