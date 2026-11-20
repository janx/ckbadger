'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { ExternalLink } from '@/components/ui/external-link';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { api } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { LANE_ORDER, VmLaneTimeline, dayStartMs } from '@/components/hardforks/vm-lane-timeline';
import { formatBlockNumber } from '@/lib/format-block';
import { formatExactDecimal } from '@/lib/exact-integer';
import { cn, formatUtc8Date } from '@/lib/utils';
import type { HardforkHashTypeLaneF2 } from '@/lib/api';
import { useDocumentTitle } from '@/lib/use-document-title';
import { formatCount } from '@/lib/format-count';
import { isAwaited } from '@/lib/query-state';
import { unitNode } from '@/components/ui/units';
import { fontPx, textPx } from '@/lib/type-metrics';

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

/*
 * A consensus row (readability plan P4; the review: nine "RFC 0029 / epoch 0" cells without titles):
 * the RFC, the epoch it switches on at and its one line, at the small step. The RFC and the epoch
 * stand in columns as wide as their widest ("RFC 0000", 62.4 px; "not activated", 101.4 px), 10 px
 * apart; the line takes the rest of the row where it has room for 32 characters (249.6 px) and
 * goes under them where it has not (a 390 phone's 358 px leaves it 174.2 px beside them: 22
 * characters a line, three lines for the longest; under them 45). The list is as wide as its
 * widest row, every line on one (62.4 + 10 + 101.4 + 10 + 63 × 7.8 = 675.2 px), so its rules end
 * where its words do.
 */
const RFC_GAP = 10;
const RFC_COLUMN = textPx('RFC 0000'.length, fontPx('small'));
const EPOCH_COLUMN = textPx('not activated'.length, fontPx('small'));
const RFC_LINE_MIN = textPx(32, fontPx('small'));
const CONSENSUS_WIDTH =
  RFC_COLUMN +
  RFC_GAP +
  EPOCH_COLUMN +
  RFC_GAP +
  textPx(Math.max(...Object.values(RFC_TEXT).map((line) => line.length)), fontPx('small'));

const DAY_MS = 86_400_000;

/**
 * The whole days from an edition's activation day (its start, UTC+8) to a later moment: a count of
 * one or more, or null when less than a day lies between them, the moment the same day or before
 * it. A story writes "N days later" / "N days on" only from a count, so it never writes a zero or
 * negative one (A4 review 1: a store still syncing has its tip before an edition).
 */
function wholeDaysAfter(since: number, ms: number): number | null {
  return ms - since >= DAY_MS ? Math.floor((ms - since) / DAY_MS) : null;
}

/** The sentence an edition's card closes with while the index's tip is before its epoch. */
const NOT_REACHED = 'The index has not reached this edition yet.';

/**
 * What each edition changed for scripts, in plain words (CH-42), with the registry's facts: the
 * first script registered under data1 and how long data2 has waited (R2b-b `hashTypes`, R11).
 * Hierarchy v3 (H3): at most two sentences; that type-hash scripts moved to the new VM, and what
 * the Spawn syscalls do, are tier 2's (`HowHashTypePicksAVm`). An edition the index has not reached
 * (its `status`, the tip's epoch against the activation epoch) counts no days to the tip and says
 * so; a data1 script deployed before Mirana is dated, not counted back.
 */
function editionStory(event: Edition, timeline: Timeline, tipMs: number | null): string {
  const data2 = timeline.hashTypeLanes!.find((lane) => lane.hashType === 'data2');
  if (!data2) throw new Error('hardfork timeline has no data2 lane');
  const since = dayStartMs(event.activationDate);
  const reached = event.status === 'activated';
  const notReached = reached ? '' : ` ${NOT_REACHED}`;
  if (event.id === 'mirana-2021') {
    const first = timeline.scriptDeployments
      .filter((deployment) => deployment.hashTypes!.includes('data1'))
      .sort((a, b) => a.timestampMs - b.timestampMs)[0];
    return `Added CKB-VM version 1 and the hash type data1 to pin it.${
      first ? ` ${firstData1(first, since)}` : ''
    }${notReached}`;
  }
  if (event.id === 'meepo-2024') {
    if (data2.knownVersions > 0)
      return `Added CKB-VM version 2, with the Spawn syscalls, and the hash type data2. ${data2.knownVersions.toLocaleString('en-US')} known script version${data2.knownVersions === 1 ? '' : 's'} use data2.${notReached}`;
    // reached: the tip is at or past the activation epoch, so the days on are the tip's
    const daysOn = reached && tipMs !== null ? wholeDaysAfter(since, tipMs) : null;
    return `Added CKB-VM version 2, with the Spawn syscalls, and the hash type data2. No script CKBadger knows uses data2 yet${
      daysOn === null ? '' : `, ${formatCount(daysOn, 'day')} on`
    }.${notReached}`;
  }
  return event.summary;
}

/** The first script registered under data1: how long after Mirana's day it came, or its date. */
function firstData1(first: Timeline['scriptDeployments'][number], since: number): string {
  if (first.timestampMs < since)
    return `The first known script pinned with data1, ${first.versionName}, was deployed before it, on ${formatUtc8Date(first.timestampMs)}.`;
  const days = wholeDaysAfter(since, first.timestampMs);
  return `The first known script pinned with data1 came ${
    days === null ? 'the same day' : `${formatCount(days, 'day')} later`
  }: ${first.versionName}.`;
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
      {/* Phase 5 (B13): the edition as a command, its status the args (dim: R11, no coloured
          words in tier 1 on a page without a figure); the box its words, as the date line's. */}
      <PromptHeading
        as="h3"
        id={headingId}
        command={event.name}
        args={event.status}
        className="text-section self-start font-medium"
      />
      {/* The edition's epoch, block and day are values: the reading ink at the small step
          (readability R3, R4; they were the quiet ink at 12 px), the block a link inside the line. */}
      <p className="text-text text-small max-w-measure self-start">
        epoch {event.activationEpoch.toLocaleString('en-US')} ·{' '}
        {event.activationBlock !== null && (
          <>
            <Link href={`/blocks/${event.activationBlock}`} look="inline">
              block {formatBlockNumber(event.activationBlock)}
            </Link>{' '}
            ·{' '}
          </>
        )}
        {unitNode(event.activationDate)}
      </p>
      <p className="text-text text-body max-w-measure leading-relaxed">
        {editionStory(event, timeline, tipMs)}
      </p>
      {rfcs.length > 0 && (
        <div className="text-small">
          {rfcs.map((feature) => (
            <div
              key={feature.rfc}
              className="border-row-rule grid grid-cols-[90px_minmax(0,1fr)] gap-2.5 border-t py-1.5"
            >
              <span className="text-text-bright">RFC {feature.rfc}</span>
              <span className="text-text">{RFC_TEXT[feature.rfc] ?? ''}</span>
            </div>
          ))}
        </div>
      )}
      {/* Every one leaves the site, so every one says so (U-20): ` ↗`, a new tab. Links inside
          the line, in its reading ink (R11: no coloured words in this page's tier 1). */}
      <div className="text-text text-small flex flex-wrap gap-x-5 gap-y-1">
        {overview && (
          <ExternalLink href={overview.url} look="inline">
            {overview.label} overview
          </ExternalLink>
        )}
        {resources.map((resource) => (
          <ExternalLink key={resource.url} href={resource.url} look="inline">
            {resource.label}
          </ExternalLink>
        ))}
      </div>
    </article>
  );
}

/**
 * A tier-1 section (R6), as the DAO page heads its sections: the h2 at section size in its own row,
 * 18 px above the evidence — a command line (rubbing × Song print, phase 5), the title as the
 * command and what it reads as the args. The section is named by its title.
 */
function Section({
  title,
  args,
  className,
  children,
}: {
  title: string;
  /** What the section is read from: `mainnet · epoch 15,265`. */
  args?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className={cn('min-w-0', className)}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <PromptHeading command={title} args={args} />
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
    <p className="text-text text-body max-w-measure mt-1.5">
      Editions change CKB&apos;s rules; a script&apos;s hash type picks its VM.
    </p>
  );
}

/** The caption's link down to the explanation (and the tier-2 anchor it lands on). */
const HOW_ID = 'how-hash-type-picks-a-vm';

/**
 * Tier 2 (hierarchy v3): the explanation tier 1 states as a timeline and one caption — the opening
 * paragraph the page used to lead with (CH-43) and the paragraph that stood under the timeline,
 * with every lane's live references (CH-41). Two columns from lg, prose on the measure (76
 * characters, readability R4).
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
        <div className="max-w-measure flex min-w-0 flex-col gap-3.5">
          <p className="text-text text-body">
            CKB changes its rules in editions. Each one switches on at an agreed epoch, and every
            node must run it from there on. Both so far brought a new version of CKB-VM, the machine
            that runs scripts, and a cell&apos;s scripts choose which one runs them through their
            hash type.
          </p>
          {/* Moved from the edition cards (H3): what each edition did to type-hash scripts. */}
          <p className="text-text text-body">
            Scripts named by type hash moved to VM1 at {vm1.shortName}&apos;s epoch,{' '}
            {vm1.activationEpoch.toLocaleString('en-US')}, and to VM2 at {vm2.shortName}&apos;s,{' '}
            {vm2.activationEpoch.toLocaleString('en-US')}. {vm2.shortName}&apos;s Spawn syscalls let
            scripts start and talk to other scripts.
          </p>
        </div>
        <p className="text-text text-body max-w-measure">
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
 * Tier 2: where the consensus rows come from (the review: the section's subtitle was the RPC's
 * method name, "get_consensus · hardfork_features · mainnet", in tier 1).
 */
function WhereConsensusComesFrom({ network }: { network: string }) {
  return (
    <DetailSection title="Where the consensus rows come from">
      <p className="text-text text-body max-w-measure">
        The consensus rows are what this node’s get_consensus RPC answers as its hardfork_features
        on {network}: each RFC and the epoch it switches on at.
      </p>
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

const HARDFORKS_TITLE = ['Hardforks'] as const;

export default function HardforksPage() {
  useDocumentTitle(HARDFORKS_TITLE);
  const network = useActiveNetwork();
  const read = useQuery({
    queryKey: ['hardforks', network],
    queryFn: () => api.getHardforks({ network }),
    staleTime: 60_000,
  });
  const { data, error } = read;
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
    <div className="min-h-screen">
      <main className="container mx-auto pb-16 pt-8">
        <div className="text-body min-w-0 font-mono">
          {/* Hierarchy v3 (R13): the hero is tier 1's first section — the title row, one sentence,
              then the VM timeline as the page's display element (it has no defining number). */}
          <div data-tier="1" className="min-w-0">
            <section aria-label="Hardforks" className="min-w-0">
              <TitleRow
                status={
                  data && (
                    <span className="text-text-dim text-small">
                      {data.network} · tip epoch {data.tipEpoch.toLocaleString('en-US')}
                    </span>
                  )
                }
              />
              <Lede />
              {/* isAwaited: the read is waited for while paused offline too (A1 round 2) */}
              {isAwaited(read) && <p className="text-text mt-8">Loading hardfork timeline…</p>}
              {error && (
                <p role="alert" className="text-negative max-w-measure mt-8">
                  Failed to load hardfork timeline: {String(error)}
                </p>
              )}
              {data && (!mirana || !meepo) && (
                <p role="alert" className="text-negative mt-8">
                  Consensus edition timeline is incomplete.
                </p>
              )}
              {data && !lanesValid && (
                <p role="alert" className="text-negative max-w-measure mt-8">
                  The hardfork timeline has no hash-type lanes in VM order (type, data1, data2,
                  data); the live references per hash type cannot be shown.
                </p>
              )}
              {data && unlaned && (
                <p role="alert" className="text-negative max-w-measure mt-8">
                  Script deployment {unlaned.versionHash} has no registered hash types; the versions
                  cannot be placed on their lanes.
                </p>
              )}
              {ready && (
                <div className="mt-8 min-w-0">
                  {ready.span.genesisMs === null || ready.span.tipMs === null ? (
                    <p className="text-text text-small">
                      The index holds no block header yet; the timeline draws once it does.
                    </p>
                  ) : ready.span.tipMs <= dayStartMs(ready.meepo.activationDate) ? (
                    <p className="text-text text-small max-w-measure">
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
              <p className="text-text text-body max-w-measure mt-3.5">
                A script named by its type hash follows every upgrade; one named by its data hash
                stays on the VM its hash type pins.{' '}
                {/* A way onward, down this page: the look writes its ↓ (components/ui/link.tsx). */}
                <Link href={`#${HOW_ID}`} look="onward">
                  How it works
                </Link>
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
              // Phase 5 (B14): the node's consensus, read on its network at the tip epoch, written
              // as the title row writes them.
              <Section
                title="consensus"
                args={`${ready.data.network} · epoch ${ready.data.tipEpoch.toLocaleString('en-US')}`}
                className="mt-[72px]"
              >
                {/* One row per RFC: its number, its epoch and its one line (RFC_TEXT, the
                    page's one wording of an RFC, the edition cards'); RFC 0038 keeps its number
                    alone, its title never verified. The RPC they come from is said in tier 2. */}
                <dl className="text-small" style={{ maxWidth: CONSENSUS_WIDTH }}>
                  {ready.data.consensusFeatures.map((feature) => (
                    <div
                      key={feature.rfc}
                      className="border-row-rule flex flex-wrap border-t py-1.5"
                      style={{ columnGap: RFC_GAP }}
                    >
                      <dt className="text-text-bright shrink-0" style={{ width: RFC_COLUMN }}>
                        RFC {feature.rfc}
                      </dt>
                      <dd className="text-text shrink-0" style={{ width: EPOCH_COLUMN }}>
                        {feature.activationEpoch === null
                          ? 'not activated'
                          : `epoch ${feature.activationEpoch.toLocaleString('en-US')}`}
                      </dd>
                      {RFC_TEXT[feature.rfc] && (
                        <dd
                          className="text-text min-w-0 flex-1"
                          style={{ flexBasis: RFC_LINE_MIN }}
                        >
                          {RFC_TEXT[feature.rfc]}
                        </dd>
                      )}
                    </div>
                  ))}
                </dl>
                {genesisFeatures.length > 0 && (
                  <p className="text-text text-body max-w-measure mt-3">
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
                <WhereConsensusComesFrom network={ready.data.network} />
              </DetailTier2>
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
