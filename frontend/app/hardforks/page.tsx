'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { api, type HardforkHashTypeLaneF2 } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { LANE_ORDER, VmLaneTimeline } from '@/components/hardforks/vm-lane-timeline';
import { formatBlockNumber } from '@/lib/format-block';
import { formatExactDecimal } from '@/lib/exact-integer';

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

/** What each edition changed for scripts, in plain words (CH-42). */
function editionStory(event: Edition, lanes: readonly HardforkHashTypeLaneF2[]): string {
  const data2 = lanes.find((lane) => lane.hashType === 'data2');
  if (!data2) throw new Error('hardfork timeline has no data2 lane');
  if (event.id === 'mirana-2021')
    return 'Added CKB-VM version 1 and the hash type data1 to pin it. Scripts named by type hash moved to VM1 at this epoch.';
  if (event.id === 'meepo-2024')
    return `Added CKB-VM version 2, with the Spawn syscalls (scripts can start and talk to other scripts), and the hash type data2. Scripts named by type hash moved to VM2. ${
      data2.knownVersions === 0
        ? 'No script CKBadger knows uses data2 yet.'
        : `${data2.knownVersions.toLocaleString()} known script version${data2.knownVersions === 1 ? '' : 's'} use data2.`
    }`;
  return event.summary;
}

function joinWords(items: readonly string[]): string {
  return items.length <= 1
    ? items.join('')
    : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

function EditionCard({ event, timeline }: { event: Edition; timeline: Timeline }) {
  const lanes = timeline.hashTypeLanes!;
  const rfcs = timeline.consensusFeatures.filter(
    (feature) => feature.activationEpoch === event.activationEpoch
  );
  const [overview, ...resources] = event.resources;
  const headingId = `edition-${event.id}`;
  return (
    <article aria-labelledby={headingId} className="flex min-w-0 flex-col gap-2">
      <div className="flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
        <h3 id={headingId} className="text-text-bright text-[15px] font-medium">
          {event.name}
        </h3>
        <span className="text-jade text-[12px]">{event.status}</span>
      </div>
      <p className="text-text-dim text-[12px]">
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
      <p className="text-text text-[13px] leading-relaxed">{editionStory(event, lanes)}</p>
      {rfcs.length > 0 && (
        <div className="text-[12px]">
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
      <div className="flex flex-wrap gap-x-5 gap-y-1 text-[12px]">
        {overview && (
          <a
            href={overview.url}
            target="_blank"
            rel="noreferrer"
            className="text-emphasis hover:underline"
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
            className="text-emphasis hover:underline"
          >
            {resource.label}
          </a>
        ))}
      </div>
    </article>
  );
}

/** The genesis and tip header times bound the axis; both are ordinary block reads. */
function useChainSpan(tipBlock: number | undefined) {
  const genesis = useQuery({
    queryKey: ['block', '0'],
    queryFn: () => api.getBlock(0),
    staleTime: Infinity,
  });
  const tip = useQuery({
    queryKey: ['block', String(tipBlock)],
    queryFn: () => api.getBlock(tipBlock!),
    enabled: tipBlock !== undefined,
    staleTime: 60_000,
  });
  const ms = (timestamp: string | undefined, which: string) => {
    if (timestamp === undefined) return null;
    const value = new Date(timestamp).getTime();
    if (!Number.isFinite(value)) throw new Error(`invalid ${which} header time ${timestamp}`);
    return value;
  };
  return {
    genesisMs: ms(genesis.data?.timestamp, 'genesis'),
    tipMs: ms(tip.data?.timestamp, 'tip'),
    error: genesis.error ?? tip.error,
  };
}

export default function HardforksPage() {
  const network = useActiveNetwork();
  const { data, isLoading, error } = useQuery({
    queryKey: ['hardforks', network],
    queryFn: () => api.getHardforks({ network }),
    staleTime: 60_000,
  });
  const span = useChainSpan(data?.tipBlock);
  const events = data?.events ?? [];
  const mirana = events.find((event) => event.id === 'mirana-2021');
  const meepo = events.find((event) => event.id === 'meepo-2024');
  const lanes = data?.hashTypeLanes;
  const lanesValid =
    lanes !== undefined && lanes.map((lane) => lane.hashType).join() === LANE_ORDER.join();
  const genesisFeatures =
    data?.consensusFeatures.filter((feature) => feature.activationEpoch === 0) ?? [];
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <DetailLayout
          title="Hardforks"
          kind="topic"
          // The board's lead, in plain words (CH-43).
          identity={
            <p className="text-text max-w-[820px] text-[15px] leading-relaxed">
              CKB changes its rules in editions. Each one switches on at an agreed epoch, and every
              node must run it from there on. Both so far brought a new version of CKB-VM, the
              machine that runs scripts, and a cell&apos;s scripts choose which one runs them
              through their hash type.
            </p>
          }
          status={
            data && (
              <span className="text-text-dim text-xs">
                {data.network} · tip epoch {data.tipEpoch.toLocaleString()}
              </span>
            )
          }
        >
          {isLoading && <p className="text-text-dim">Loading hardfork timeline…</p>}
          {error && (
            <p role="alert" className="text-negative">
              Failed to load hardfork timeline: {String(error)}
            </p>
          )}
          {data && (!mirana || !meepo) && (
            <p role="alert" className="text-negative">
              Consensus edition timeline is incomplete.
            </p>
          )}
          {data && !lanesValid && (
            <p role="alert" className="text-negative">
              The hardfork timeline has no hash-type lanes in VM order (type, data1, data2, data);
              the live references per hash type cannot be shown.
            </p>
          )}
          {data && mirana && meepo && lanesValid && (
            <>
              <DetailSection
                title="Which VM runs a script, over time"
                actions={
                  <span className="text-text-dim text-[12px]">
                    dots: when each known script version was deployed · hover for its name
                  </span>
                }
              >
                {span.error ? (
                  <p role="alert" className="text-negative text-[12px]">
                    Chain time span unavailable: {span.error.message}
                  </p>
                ) : span.genesisMs === null || span.tipMs === null ? (
                  <p className="text-text-dim text-[12px]">Loading the chain&apos;s time span…</p>
                ) : span.tipMs <= dayStartGuard(meepo.activationDate) ? (
                  <p className="text-text-dim text-[12px]">
                    The index has not reached the latest edition yet; the timeline draws once it
                    has.
                  </p>
                ) : (
                  <VmLaneTimeline
                    editions={[mirana, meepo]}
                    lanes={lanes!}
                    deployments={data.scriptDeployments}
                    genesisMs={span.genesisMs}
                    tipMs={span.tipMs}
                    tipEpoch={data.tipEpoch}
                  />
                )}
                <p className="text-text mt-4 max-w-[940px] text-[12.5px] leading-relaxed">
                  A script named by its type hash follows every upgrade: the same code runs on VM0,
                  then VM1, then VM2. A script named by its data hash stays on the VM its hash type
                  pins, so a hardfork never changes how an existing data-pinned script runs. Live
                  cells reference registered scripts{' '}
                  {formatExactDecimal(lanes![0].liveReferences, 'type live references')} times by
                  type hash, {formatExactDecimal(lanes![1].liveReferences, 'data1 live references')}{' '}
                  by data1, {formatExactDecimal(lanes![2].liveReferences, 'data2 live references')}{' '}
                  by data2 and{' '}
                  {formatExactDecimal(lanes![3].liveReferences, 'data live references')} by data.
                </p>
              </DetailSection>
              <div className="grid min-w-0 items-start gap-12 md:grid-cols-2">
                {events.map((event) => (
                  <EditionCard key={event.id} event={event} timeline={data} />
                ))}
              </div>
              <DetailSection title="As this node's consensus states it">
                <p className="text-text-dim mb-3 text-[12px]">
                  get_consensus · hardfork_features · {data.network}
                </p>
                <dl className="grid grid-cols-3 gap-2 sm:grid-cols-5 lg:grid-cols-9">
                  {data.consensusFeatures.map((feature) => (
                    <div
                      key={feature.rfc}
                      className="border-base-border-subtle border-t py-2 text-[12px]"
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
                  <p className="text-text-dim mt-3 text-[12px]">
                    Today&apos;s node applies RFC{' '}
                    {joinWords(genesisFeatures.map((feature) => feature.rfc))} from epoch 0, so they
                    do not show as switching on with an edition.
                  </p>
                )}
              </DetailSection>
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}

/** The start of an edition's activation day (UTC+8), for the "not reached yet" guard. */
function dayStartGuard(day: string): number {
  const ms = Date.parse(`${day}T00:00:00+08:00`);
  if (!Number.isFinite(ms)) throw new Error(`Invalid hardfork activation date ${day}`);
  return ms;
}
