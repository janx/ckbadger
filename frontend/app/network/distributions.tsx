'use client';

import { useQuery } from '@tanstack/react-query';
import { api, LabelCount } from '@/lib/api';
import { DATA_COLOR } from '@/lib/design-tokens';

const TOP_N = 8;
/** The hosting bar's segments: one hue, the largest strongest (Network board). */
const PROVIDER_OPACITY = [0.9, 0.6, 0.5, 0.42, 0.34, 0.2] as const;

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
  if (top.length === 0) return <p className="text-text-dim text-xs">No data yet</p>;
  return (
    <div className="flex max-w-[720px] flex-col gap-1">
      {top.map((it, i) => (
        <div
          key={it.label}
          className="grid grid-cols-[minmax(0,1fr)_90px_36px] items-center gap-2.5 py-0.5 text-[12px]"
        >
          <span
            className={`truncate ${i === 0 ? 'text-text-bright' : 'text-text'}`}
            title={it.label}
          >
            {it.label}
          </span>
          <span aria-hidden="true" className="bg-base-elevated flex h-1.5 rounded-[3px]">
            <span
              {...{ [attr]: '' }}
              className="rounded-[3px]"
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
      <p role="alert" className="text-negative text-[12px]">
        Failed to load distributions.
      </p>
    );
  return (
    <section className="flex min-w-0 flex-col gap-3">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-[15px] font-medium">Which software they run</h2>
        <span className="text-text-dim text-[12px]">reachable now or verified before</span>
      </div>
      <Bars items={data.versions} attr="data-version-bar" />
    </section>
  );
}

/** The protocols the verified peers speak: developer evidence (tier 2, CH-54). */
export function NetworkProtocols() {
  const { data, isLoading, error } = useDistributions();
  if (isLoading) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-[12px]">
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
 * as one (Amazon's AS16509 and AS14618) — the real centralisation risk (CH-52).
 */
export function HostingBar() {
  const { data, isLoading, error } = useDistributions();
  if (isLoading) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-[12px]">
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
      <p role="alert" className="text-negative text-[12px]">
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
    <div className="flex flex-col gap-2">
      <span className="text-text-bright text-[12.5px]">Where they run</span>
      <div
        role="img"
        aria-label="Verified nodes by hosting network"
        className="flex h-3 gap-0.5 overflow-hidden rounded-md"
      >
        {segments.map(([org, count], i) => (
          <span
            key={org}
            data-provider={org}
            style={{
              flex: `${count} 0 0`,
              backgroundColor: DATA_COLOR,
              opacity: PROVIDER_OPACITY[i],
            }}
          />
        ))}
      </div>
      <div className="text-text-dim flex flex-wrap gap-x-5 gap-y-1 text-[12px]">
        {segments.map(([org, count]) => (
          <span key={org}>
            {org} <span className="text-text-bright">{count.toLocaleString()}</span>
          </span>
        ))}
      </div>
      {leader && (
        <span className="text-text-dim text-[12px]">
          {leader[1].toLocaleString()} of the {total.toLocaleString()} verified nodes run on{' '}
          {leader[0]}, across {countries} countries.
        </span>
      )}
    </div>
  );
}
