'use client';

import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import { api, LabelCount, type NetworkDistributions as Distributions } from '@/lib/api';
import { DATA_COLOR, DATA_KEY } from '@/lib/design-tokens';
import { color, fade, inkOnFill } from '@/lib/palette';
import { useTheme } from '@/hooks/useTheme';
import { formatCount } from '@/lib/format-count';
import { partitionSpans } from '@/lib/partition-geometry';
import { useElementWidth } from '@/hooks/useElementWidth';
import { TopicSection } from '@/app/network/topic-section';
import { percentOf } from '@/lib/format-unit';
import { unitNode } from '@/components/ui/units';
import { fontPx, foreignTextPx } from '@/lib/type-metrics';

const TOP_N = 8;
/**
 * The hosting bar's segments: one hue, the largest strongest (Network board). DY-03: each step
 * keeps the jade where one of the two fill inks reads at 4.5:1 in both themes (0.6 and 0.5 sat in
 * the mid-tone band where neither does by night: 4.24 and 4.36:1).
 */
export const PROVIDER_OPACITY = [0.9, 0.66, 0.46, 0.38, 0.3, 0.2] as const;
/** The in-segment words (hierarchy v3 R15): the small step (`text-small`), 10 px padding each side. */
const LABEL_PAD = 10;

/** The data hue at `alpha`, as a background, so a segment's words keep their full ink. */
function dataHue(alpha: number): string {
  return fade(DATA_COLOR, alpha);
}

/**
 * The three parts wait while the read is awaited — fetching, or paused (offline, or a hidden tab
 * between retries), which isLoading leaves out — and only then read its error or data.
 */
function useDistributions() {
  // Shares the ['network', 'distributions'] key with the MaxMind attribution: one fetch.
  return useQuery({
    queryKey: ['network', 'distributions'],
    queryFn: api.getNetworkDistributions,
    refetchInterval: 30000,
  });
}

/**
 * A grouping the distributions route states for this page (lane exactness, part A, A5 iii); an API older than the
 * page sends none, and the part says so in place.
 */
function grouping<K extends 'hosting' | 'releases'>(
  data: Distributions,
  field: K
): Distributions[K] | string {
  const value = (data as Partial<Distributions>)[field];
  return value === undefined
    ? `Network distributions send no ${field}: the API is older than this page.`
    : (value as Distributions[K]);
}

/**
 * The shown rows with one release's builds together (P4 topics: one release's builds stood apart,
 * by count), in the route's releases' order, the largest release first, each release's builds by
 * count (lane exactness, part A, A5 iii: the release of a build is the route's, read from its `releases`). A
 * row's count is the API's; the page totals nothing.
 */
function grouped(rows: readonly LabelCount[], releases: Distributions['releases']): LabelCount[] {
  const shown = new Set(rows.map((row) => row.label));
  return releases.flatMap((release) => release.builds.filter((build) => shown.has(build.label)));
}

/**
 * A top-N bar list in the data hue, the leader strongest (Network board). Labels stay DOM text;
 * `attr` marks the bars for their own section. One grid for every row (P4 topics: the names stood
 * at the list's left, their bars 340 px away): the names' column as wide as the widest name (a
 * client version, "0.210.0 (2592ddf 2026-09-17)", 28 characters at 13 px, 218.4 px), its bar 10 px
 * after it, its count after that; a name wider than the line is cut, whole in its title.
 */
function Bars({
  items,
  attr,
  label,
  group,
}: {
  items: LabelCount[];
  attr: string;
  /** The list's name, for a screen reader. */
  label: string;
  /** Rows of one release stand together (a release's builds): the route's releases. */
  group?: Distributions['releases'];
}) {
  const top = [...items].sort((a, b) => b.count - a.count).slice(0, TOP_N);
  const max = top.reduce((m, it) => Math.max(m, it.count), 0);
  if (top.length === 0) return <p className="text-text text-small">No data.</p>;
  const rows = group === undefined ? top : grouped(top, group);
  return (
    <ul
      aria-label={label}
      className="text-small grid w-fit max-w-full grid-cols-[minmax(0,max-content)_90px_max-content] items-center gap-x-2.5 gap-y-1"
    >
      {rows.map((it) => {
        const leader = it === top[0];
        return (
          <li key={it.label} className="contents">
            <span
              className={`truncate ${leader ? 'text-text-bright' : 'text-text'}`}
              title={it.label}
            >
              {it.label}
            </span>
            <span aria-hidden="true" className="bg-bar-track flex h-1.5">
              <span
                {...{ [attr]: '' }}
                className="scan-fill"
                style={{
                  width: `${(100 * it.count) / max}%`,
                  backgroundColor: DATA_COLOR,
                  opacity: leader ? 0.9 : 0.5,
                }}
              />
            </span>
            <span className="text-text text-right tabular-nums">
              {it.count.toLocaleString('en-US')}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

/** "Which software they run": the client versions of the verified peers (CH-54). */
export function NetworkDistributions() {
  const read = useDistributions();
  const { data, error } = read;
  if (isAwaited(read)) return <div className="bg-base-elevated h-40 w-full animate-pulse" />;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Failed to load distributions.
      </p>
    );
  const releases = grouping(data, 'releases');
  if (typeof releases === 'string')
    return (
      <p role="alert" className="text-negative text-small max-w-measure">
        {releases}
      </p>
    );
  // Its scope ("reachable now or verified before") is said in tier 2 (R14: no note beside the h2).
  return (
    <TopicSection title="Which software they run" args={formatCount(data.verifiedRetained, 'node')}>
      <Bars
        items={data.versions}
        attr="data-version-bar"
        label="Client versions"
        group={releases}
      />
    </TopicSection>
  );
}

/** The protocols the verified peers speak: developer evidence (tier 2, CH-54). */
export function NetworkProtocols() {
  const read = useDistributions();
  const { data, error } = read;
  if (isAwaited(read)) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Failed to load protocol counts.
      </p>
    );
  return <Bars items={data.protocols} attr="data-protocol-bar" label="Protocols" />;
}

/**
 * "Where they run": verified nodes by hosting organisation, so one provider's several ASNs read
 * as one (Amazon's AS16509 and AS14618) — the real centralisation risk (CH-52). Hierarchy v3: a
 * 28 px bar with each organisation's name and share inside its segment where they fit (else the
 * share alone, else nothing), the exact counts in one quiet line under it, then one caption.
 */
export function HostingBar() {
  const read = useDistributions();
  const { data, error } = read;
  if (isAwaited(read)) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Failed to load hosting networks.
      </p>
    );
  if (data.verifiedRetained === 0) return null;
  // the organisations and their rest, the route's (lane exactness, part A, A5 iii: the page summed the ASNs by
  // organisation and the rest); they make the verified nodes, which the route checks
  const hosting = grouping(data, 'hosting');
  if (typeof hosting === 'string')
    return (
      <p role="alert" className="text-negative text-small max-w-measure">
        {hosting}
      </p>
    );
  const total = data.verifiedRetained;
  const shown = hosting.organisations.map((org) => [org.label, org.count] as const);
  const segments = hosting.rest > 0 ? [...shown, ['other networks', hosting.rest] as const] : shown;
  const leader = shown.find(([org]) => org !== 'Unknown');
  // the countries are counted once, on the map, of its located nodes (lane exactness, part A2, the pages review's
  // NOTE 3: this said "across C countries" of every verified node, beside the map's own count)
  return (
    <HostingSection
      segments={segments}
      total={total}
      caption={
        leader &&
        `${leader[1].toLocaleString('en-US')} of the ${total.toLocaleString('en-US')} verified nodes run on ${leader[0]}.`
      }
    />
  );
}

/** The segments of the hosting bar stand this far apart (`gap-0.5`). */
const SEGMENT_GAP_PX = 2;
/** A part's swatch under the bar and the room after it (`h-[9px] w-[9px]`, `mr-1.5`). */
const SWATCH_PX = 9;
const SWATCH_GAP_PX = 6;

/**
 * Where a part named under the bar stands (readability plan 2026-11-18, P4: "a split bar names
 * every segment in place"): its line starts at the part's left edge where its words fit from there
 * to the bar's end, else it ends at the part's right edge, else (words wider than the bar up to the
 * part's end, a narrow phone) it starts at the bar's start and wraps inside the bar's width. Never
 * before the bar's start, never past its end.
 */
function underLineLeft(left: number, right: number, words: number, bar: number): number {
  if (left + words <= bar) return left;
  if (words <= right) return right - words;
  return 0;
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
  const { resolved: theme } = useTheme();
  const counts = segments.map(([, count]) => BigInt(count));
  // The segments grow by their parts from no width (`flex-grow`), so the bar's width less its gaps
  // is shared by the parts exactly as `partitionSpans` shares it: the lines under the bar stand at
  // the segments' own edges.
  const weights = partitionSpans(counts, BigInt(total), 0, 100, 'hosting networks');
  const gaps = SEGMENT_GAP_PX * (segments.length - 1);
  const spans =
    width === null
      ? null
      : partitionSpans(counts, BigInt(total), 0, width - gaps, 'hosting networks').map(
          (span, i) => ({ left: span.x + SEGMENT_GAP_PX * i, width: span.width })
        );
  const share = (count: number) => percentOf(BigInt(count), BigInt(total));
  /** A part's words, said once: its name, its share and its count. */
  const words = (org: string, count: number) =>
    `${org} ${share(count)} · ${formatCount(count, 'node')}`;
  // An organisation is the ASN registry's text, in any script: measured by lib/type-metrics.ts's
  // count for text that is not ours (not an upper bound: a few graphemes draw wider), and held by
  // its room either way: clipped at its segment's edge, wrapped inside the bar under it.
  const wordsPx = (org: string, count: number) => foreignTextPx(words(org, count), fontPx('small'));
  /** Whether a part's segment holds its words, 10 px each side. */
  const inside = (org: string, count: number, i: number): boolean =>
    spans !== null && wordsPx(org, count) + 2 * LABEL_PAD <= spans[i].width;
  const under =
    spans === null
      ? []
      : segments.flatMap(([org, count], i) => {
          if (inside(org, count, i)) return [];
          const line = SWATCH_PX + SWATCH_GAP_PX + wordsPx(org, count);
          const { left, width: part } = spans[i];
          return [{ org, count, i, left: underLineLeft(left, left + part, line, width!) }];
        });
  return (
    <TopicSection title="Where they run" args={formatCount(total, 'node')}>
      <div
        ref={ref}
        role="img"
        aria-label="Verified nodes by hosting network"
        title={segments.map(([org, count]) => `${org} ${share(count)}`).join(' · ')}
        className="text-small flex h-[28px] w-full gap-0.5 overflow-hidden tabular-nums"
      >
        {segments.map(([org, count], i) => (
          <span
            key={org}
            data-provider={org}
            className="flex min-w-0 items-center overflow-hidden whitespace-nowrap px-2.5"
            style={{
              flexGrow: weights[i].width,
              flexShrink: 1,
              flexBasis: 0,
              backgroundColor: dataHue(PROVIDER_OPACITY[i]),
              // DY-03: the words take the ink their fill's own luminance calls for, in this theme.
              color: color(inkOnFill(theme, DATA_KEY, PROVIDER_OPACITY[i]).ink),
            }}
          >
            {inside(org, count, i) && <span>{words(org, count)}</span>}
          </span>
        ))}
      </div>
      {under.length > 0 && (
        // Each part that does not hold its words is named on a line of its own under the bar, at
        // the part, after a swatch of its own fill: one line a part, in the bar's order.
        <ul aria-label="Hosting networks named under the bar" className="text-small mt-1.5">
          {under.map(({ org, count, i, left }) => (
            <li
              key={org}
              data-provider-line={org}
              className="text-text w-fit [overflow-wrap:anywhere]"
              style={{ marginLeft: `${left}px`, maxWidth: `${width! - left}px` }}
            >
              <span
                aria-hidden="true"
                data-role="swatch"
                className="mr-1.5 inline-block h-[9px] w-[9px] align-middle"
                style={{ backgroundColor: dataHue(PROVIDER_OPACITY[i]) }}
              />
              {org} <span className="tabular-nums">{share(count)}</span>
              <span className="text-text-dim"> · </span>
              <span className="tabular-nums">{unitNode(formatCount(count, 'node'))}</span>
            </li>
          ))}
        </ul>
      )}
      {caption && (
        <p data-role="caption" className="text-text text-body max-w-measure mt-3.5">
          {caption}
        </p>
      )}
    </TopicSection>
  );
}
