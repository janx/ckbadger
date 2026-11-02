'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { Header } from '@/components/layout/header';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { api } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { LANE_ORDER, VmLaneTimeline, dayStartMs } from '@/components/hardforks/vm-lane-timeline';
import { formatBlockNumber } from '@/lib/format-block';
import { formatExactDecimal } from '@/lib/exact-integer';
import { cn } from '@/lib/utils';
import type { HardforkHashTypeLaneF2 } from '@/lib/api';

type Timeline = Awaited<ReturnType<typeof api.getHardforks>>;
type Edition = Timeline['events'][number];

/**
 * One line per consensus RFC, in plain words (the Hardforks board). RFC 0038 has no line: its
 * title was never verified, so it shows by number alone.
 */
const RFC_TEXT: Record<string, string> = {
  '0028': 'relative since by timestamp uses the median time of past blocks',
  '0029': 'several dep cells with identical code may match one reference',
  '0030': 'since: the epoch index must be less than the epoch length',
  '0031': 'blocks gain a variable-length extension field',
  '0032': 'scripts choose their VM version through hash_type',
  '0036': 'header deps no longer need to be mature',
  '0048': 'the block header version field is no longer reserved',
  '0049': 'CKB-VM version 2',
};

const DAY_MS = 86_400_000;

/**
 * What each edition changed for scripts, in plain words (CH-42), with the registry's facts: the
 * first script registered under data1 and how long data2 has waited (R2b-b `hashTypes`, R11).
 * Hierarchy v3 (H3): at most two sentences; that type-hash scripts moved to the new VM, and what
 * the Spawn syscalls do, are tier 2's (`HowHashTypePicksAVm`).
 */
function editionStory(event: Edition, timeline: Timeline, tipMs: number | null): string {
  const data2 = timeline.hashTypeLanes!.find((lane) => lane.hashType === 'data2');
  if (!data2) throw new Error('hardfork timeline has no data2 lane');
  const since = dayStartMs(event.activationDate);
  if (event.id === 'mirana-2021') {
    const first = timeline.scriptDeployments
      .filter((deployment) => deployment.hashTypes!.includes('data1'))
      .sort((a, b) => a.timestampMs - b.timestampMs)[0];
    return `Added CKB-VM version 1 and the hash type data1 to pin it.${
      first
        ? ` The first known script pinned with data1 came ${Math.floor((first.timestampMs - since) / DAY_MS).toLocaleString()} days later: ${first.versionName}.`
        : ''
    }`;
  }
  if (event.id === 'meepo-2024')
    return `Added CKB-VM version 2, with the Spawn syscalls, and the hash type data2. ${
      data2.knownVersions === 0
        ? `No script CKBadger knows uses data2 yet${tipMs === null ? '' : `, ${Math.floor((tipMs - since) / DAY_MS).toLocaleString()} days on`}.`
        : `${data2.knownVersions.toLocaleString()} known script version${data2.knownVersions === 1 ? '' : 's'} use data2.`
    }`;
  return event.summary;
}

function joinWords(items: readonly string[]): string {
  return items.length <= 1
    ? items.join('')
    : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

function EditionCard({
  event,
  timeline,
  tipMs,
}: {
  event: Edition;
  timeline: Timeline;
  tipMs: number | null;
}) {
  const rfcs = timeline.consensusFeatures.filter(
    (feature) => feature.activationEpoch === event.activationEpoch
  );
  const [overview, ...resources] = event.resources;
  const headingId = `edition-${event.id}`;
  return (
    <article aria-labelledby={headingId} className="flex min-w-0 flex-col gap-2">
      <div className="flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
        <h3 id={headingId} className="text-text-bright text-section font-medium">
          {event.name}
        </h3>
        {/* R11: no coloured words in tier 1 on a page without a figure. */}
        <span className="text-text-dim text-small">{event.status}</span>
      </div>
      <p className="text-text-dim text-meta self-start">
        epoch {event.activationEpoch.toLocaleString()} ·{' '}
        {event.activationBlock !== null && (
          <>
            <Link href={`/blocks/${event.activationBlock}`} className="hover:text-jade">
              block {formatBlockNumber(event.activationBlock)}
            </Link>{' '}
            ·{' '}
          </>
        )}
        {event.activationDate}
      </p>
      <p className="text-text text-body max-w-[640px] leading-relaxed">
        {editionStory(event, timeline, tipMs)}
      </p>
      {rfcs.length > 0 && (
        <div className="text-small">
          {rfcs.map((feature) => (
            <div
              key={feature.rfc}
              className="border-base-border-subtle grid grid-cols-[90px_minmax(0,1fr)] gap-2.5 border-t py-1.5"
            >
              <span className="text-text-bright">RFC {feature.rfc}</span>
              <span className="text-text">{RFC_TEXT[feature.rfc] ?? ''}</span>
            </div>
          ))}
        </div>
      )}
      <div className="text-small flex flex-wrap gap-x-5 gap-y-1">
        {overview && (
          <a
            href={overview.url}
            target="_blank"
            rel="noreferrer"
            className="text-text hover:text-text-bright hover:underline"
          >
            {overview.label} overview ↗
          </a>
        )}
        {resources.map((resource) => (
          <a
            key={resource.url}
            href={resource.url}
            target="_blank"
            rel="noreferrer"
            className="text-text hover:text-text-bright hover:underline"
          >
            {resource.label}
          </a>
        ))}
      </div>
    </article>
  );
}

/**
 * A tier-1 section (R6), as the DAO page heads its sections: the h2 at section size in its own row,
 * 18 px above the evidence.
 */
function Section({
  title,
  className,
  children,
}: {
  title: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className={cn('min-w-0', className)}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">{title}</h2>
      </div>
      {children}
    </section>
  );
}

/** The page title (index step) and, once the timeline is loaded, its network and tip epoch. */
function TitleRow({ status }: { status?: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
      <h1 className="text-text-bright text-index break-words font-medium">Hardforks</h1>
      {status}
    </div>
  );
}

/**
 * Hierarchy v3 (R13): one short sentence (≤ 90 characters) under the title; the whole opening
 * paragraph is tier 2's "How hash_type picks a VM".
 */
function Lede() {
  return (
    <p className="text-text-dim text-body mt-1.5 max-w-[560px]">
      Editions change CKB&apos;s rules; a script&apos;s hash type picks its VM.
    </p>
  );
}

/** The caption's link down to the explanation (and the tier-2 anchor it lands on). */
const HOW_ID = 'how-hash-type-picks-a-vm';

/**
 * Tier 2 (hierarchy v3): the explanation tier 1 states as a timeline and one caption — the opening
 * paragraph the page used to lead with (CH-43) and the paragraph that stood under the timeline,
 * with every lane's live references (CH-41). Two columns from lg, prose at most 640 px wide.
 */
function HowHashTypePicksAVm({
  lanes,
  editions: [vm1, vm2],
}: {
  lanes: readonly HardforkHashTypeLaneF2[];
  /** The edition that added VM1, then the one that added VM2. */
  editions: readonly [Edition, Edition];
}) {
  return (
    <DetailSection title="How hash_type picks a VM">
      <div className="grid items-start gap-x-14 gap-y-3.5 lg:grid-cols-2">
        <div className="flex min-w-0 max-w-[640px] flex-col gap-3.5">
          <p className="text-text-dim text-body">
            CKB changes its rules in editions. Each one switches on at an agreed epoch, and every
            node must run it from there on. Both so far brought a new version of CKB-VM, the machine
            that runs scripts, and a cell&apos;s scripts choose which one runs them through their
            hash type.
          </p>
          {/* Moved from the edition cards (H3): what each edition did to type-hash scripts. */}
          <p className="text-text-dim text-body">
            Scripts named by type hash moved to VM1 at {vm1.shortName}&apos;s epoch,{' '}
            {vm1.activationEpoch.toLocaleString()}, and to VM2 at {vm2.shortName}&apos;s,{' '}
            {vm2.activationEpoch.toLocaleString()}. {vm2.shortName}&apos;s Spawn syscalls let
            scripts start and talk to other scripts.
          </p>
        </div>
        <p className="text-text-dim text-body max-w-[640px]">
          A script named by its type hash follows every upgrade: the same code runs on VM0, then
          VM1, then VM2. A script named by its data hash stays on the VM its hash type pins, so a
          hardfork never changes how an existing data-pinned script runs. Live cells reference
          registered scripts {formatExactDecimal(lanes[0].liveReferences, 'type live references')}{' '}
          times by type hash, {formatExactDecimal(lanes[1].liveReferences, 'data1 live references')}{' '}
          by data1, {formatExactDecimal(lanes[2].liveReferences, 'data2 live references')} by data2
          and {formatExactDecimal(lanes[3].liveReferences, 'data live references')} by data.
        </p>
      </div>
    </DetailSection>
  );
}

/**
 * The genesis and tip header times bound the axis (round 3, contracts R-B · R-10: served on
 * `/hardforks` itself; both null while the store has no indexed header). Exact ms, no block read.
 */
function chainSpan(data: { genesisTimestampMs: number | null; tipTimestampMs: number | null }) {
  const { genesisTimestampMs: genesisMs, tipTimestampMs: tipMs } = data;
  if ((genesisMs === null) !== (tipMs === null))
    throw new Error(`hardforks: genesis time ${genesisMs} and tip time ${tipMs} disagree`);
  return { genesisMs, tipMs };
}

export default function HardforksPage() {
  const network = useActiveNetwork();
  const { data, isLoading, error } = useQuery({
    queryKey: ['hardforks', network],
    queryFn: () => api.getHardforks({ network }),
    staleTime: 60_000,
  });
  const span = data ? chainSpan(data) : null;
  const events = data?.events ?? [];
  const mirana = events.find((event) => event.id === 'mirana-2021');
  const meepo = events.find((event) => event.id === 'meepo-2024');
  const lanes = data?.hashTypeLanes;
  const lanesValid =
    lanes !== undefined && lanes.map((lane) => lane.hashType).join() === LANE_ORDER.join();
  // R2b-b: every deployment says which hash types it is registered under.
  const unlaned = data?.scriptDeployments.find((deployment) => !deployment.hashTypes);
  const genesisFeatures =
    data?.consensusFeatures.filter((feature) => feature.activationEpoch === 0) ?? [];
  // Everything the timeline, the cards and tier 2 read, once the response is whole.
  const ready =
    data && span && mirana && meepo && lanesValid && !unlaned
      ? { data, span, mirana, meepo, lanes: lanes! }
      : null;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <div className="text-body min-w-0 font-mono">
          {/* Hierarchy v3 (R13): the hero is tier 1's first section — the title row, one sentence,
              then the VM timeline as the page's display element (it has no defining number). */}
          <div data-tier="1" className="min-w-0">
            <section aria-label="Hardforks" className="min-w-0">
              <TitleRow
                status={
                  data && (
                    <span className="text-text-dim text-small">
                      {data.network} · tip epoch {data.tipEpoch.toLocaleString()}
                    </span>
                  )
                }
              />
              <Lede />
              {isLoading && <p className="text-text-dim mt-8">Loading hardfork timeline…</p>}
              {error && (
                <p role="alert" className="text-negative mt-8">
                  Failed to load hardfork timeline: {String(error)}
                </p>
              )}
              {data && (!mirana || !meepo) && (
                <p role="alert" className="text-negative mt-8">
                  Consensus edition timeline is incomplete.
                </p>
              )}
              {data && !lanesValid && (
                <p role="alert" className="text-negative mt-8">
                  The hardfork timeline has no hash-type lanes in VM order (type, data1, data2,
                  data); the live references per hash type cannot be shown.
                </p>
              )}
              {data && unlaned && (
                <p role="alert" className="text-negative mt-8">
                  Script deployment {unlaned.versionHash} has no registered hash types; the versions
                  cannot be placed on their lanes.
                </p>
              )}
              {ready && (
                <div className="mt-8 min-w-0">
                  {ready.span.genesisMs === null || ready.span.tipMs === null ? (
                    <p className="text-text-dim text-small">
                      The index holds no block header yet; the timeline draws once it does.
                    </p>
                  ) : ready.span.tipMs <= dayStartMs(ready.meepo.activationDate) ? (
                    <p className="text-text-dim text-small">
                      The index has not reached the latest edition yet; the timeline draws once it
                      has.
                    </p>
                  ) : (
                    <VmLaneTimeline
                      editions={[ready.mirana, ready.meepo]}
                      lanes={ready.lanes}
                      deployments={ready.data.scriptDeployments}
                      genesisMs={ready.span.genesisMs}
                      tipMs={ready.span.tipMs}
                      tipEpoch={ready.data.tipEpoch}
                    />
                  )}
                </div>
              )}
            </section>
            {ready && (
              // Footnote budget under the timeline: its readout row and this one caption (R4, R14).
              <p className="text-text text-body mt-3.5 max-w-[640px]">
                A script named by its type hash follows every upgrade; one named by its data hash
                stays on the VM its hash type pins.{' '}
                <a
                  href={`#${HOW_ID}`}
                  className="text-text-dim hover:text-text-bright whitespace-nowrap hover:underline"
                >
                  How it works ↓
                </a>
              </p>
            )}
            {/* Rhythm (R6): the hero's caption, the editions 64 px under it, the consensus 72. */}
            {ready && (
              <div className="mt-16 grid min-w-0 items-start gap-12 md:grid-cols-2">
                {events.map((event) => (
                  <EditionCard
                    key={event.id}
                    event={event}
                    timeline={ready.data}
                    tipMs={ready.span.tipMs}
                  />
                ))}
              </div>
            )}
            {ready && (
              <Section title="As this node's consensus states it" className="mt-[72px]">
                <p className="text-text-dim text-small mb-3 w-fit">
                  get_consensus · hardfork_features · {ready.data.network}
                </p>
                <dl className="grid grid-cols-3 gap-2 sm:grid-cols-5 lg:grid-cols-9">
                  {ready.data.consensusFeatures.map((feature) => (
                    <div
                      key={feature.rfc}
                      className="border-base-border-subtle text-small border-t py-2"
                    >
                      <dt className="text-text-bright">RFC {feature.rfc}</dt>
                      <dd className="text-text-dim">
                        {feature.activationEpoch === null
                          ? 'not activated'
                          : 'epoch ' + feature.activationEpoch.toLocaleString()}
                      </dd>
                    </div>
                  ))}
                </dl>
                {genesisFeatures.length > 0 && (
                  <p className="text-text-dim text-meta mt-3 max-w-[640px]">
                    Today&apos;s node applies RFC{' '}
                    {joinWords(genesisFeatures.map((feature) => feature.rfc))} from epoch 0, so they
                    do not show as switching on with an edition.
                  </p>
                )}
              </Section>
            )}
          </div>
          {ready && (
            // "How it works ↓" under the timeline lands on the evidence boundary.
            <div id={HOW_ID} className="scroll-mt-4">
              <DetailTier2>
                <HowHashTypePicksAVm lanes={ready.lanes} editions={[ready.mirana, ready.meepo]} />
              </DetailTier2>
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
