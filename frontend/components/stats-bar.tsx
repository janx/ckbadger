'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import { api, type NetworkStats } from '@/lib/api';
import { parseEpoch, type EpochInfo } from '@/lib/epoch';
import { formatBlockNumber } from '@/lib/format-block';

/**
 * The chain vitals strip under the header, on every page: block, epoch progress, block time, hash
 * rate and difficulty, then — on a page that holds the realtime socket — "● Live" at its right end
 * (Main.dc.html, TX-01). It replaces the homepage's old hero numbers, epoch card and floating LIVE
 * pill.
 *
 * From md up it is one wrapping row. Below md it is three lines grouped by meaning, never a row to
 * pan (docs/superpowers/plans/2026-11-01-narrow-widths/SPEC.md): the block with Live at the right
 * end, the epoch with its bar taking the free width, then the three mining figures — inline pairs
 * packed to the left from 640 px, three equal columns with the label over the value below it. One
 * grid places the items in document order, so from md up the same items flow as the row.
 */
export function VitalsStrip() {
  const { data: stats, error } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });
  const isConnected = useRealtimeStore((s) => s.isConnected);
  const held = useRealtimeStore((s) => s.socketHeld);

  return (
    <section
      aria-label="Chain vitals"
      data-testid="vitals-strip"
      className="grid min-w-0 flex-1 grid-cols-3 items-baseline gap-x-4 gap-y-2 py-2.5 font-mono sm:grid-cols-[max-content_max-content_max-content_1fr] md:flex md:flex-wrap md:gap-x-6 md:gap-y-2.5 md:py-3"
    >
      {error ? (
        <VitalsError message={error.message} />
      ) : stats ? (
        <Vitals stats={stats} />
      ) : (
        <VitalsLoading />
      )}
      {held && <LiveMark connected={isConnected} />}
    </section>
  );
}

/**
 * Where each item sits below md (ignored from md up, where the strip is a flex row): the block
 * spans the tracks left of Live on line 1, the epoch and any strip-wide message take a whole line,
 * and each mining figure takes one track of line 3.
 */
const PLACE = {
  block: 'col-span-2 sm:col-span-3',
  line: 'col-span-3 sm:col-span-4',
  live: 'row-start-1 col-start-3 sm:col-start-4',
} as const;

/** A mining figure: label over value below 640 px, label beside value from 640 px up. */
const STACKED = 'flex flex-col gap-px sm:flex-row sm:items-baseline sm:gap-1.5 md:gap-2';
const INLINE = 'flex items-baseline gap-1.5 md:gap-2';

const LABEL = 'text-text-dim text-meta md:text-xs';
const VALUE =
  'text-text-bright group-hover:text-jade text-body font-medium tabular-nums transition-colors';

function Vital({
  label,
  href,
  layout,
  children,
}: {
  label: string;
  href?: string;
  /** `block` is line 1's inline pair; a mining figure stacks below 640 px. */
  layout: 'block' | 'mining';
  children: ReactNode;
}) {
  const body = (
    <>
      <span className={LABEL}>{label}</span>
      <span className={VALUE}>{children}</span>
    </>
  );
  const cls = `group ${layout === 'block' ? `${INLINE} ${PLACE.block}` : STACKED}`;
  return href ? (
    <Link href={href} className={cls}>
      {body}
    </Link>
  ) : (
    <div className={cls}>{body}</div>
  );
}

const DOT_BOX = 'h-[7px] w-[7px] shrink-0 self-center rounded-full';

/**
 * The API's average block time is a seconds string ("8.43s"); the board writes the unit apart
 * ("8.43 s", TX-05). Anything else is refused, not reshaped.
 */
function BlockTime({ value }: { value: string | null }) {
  if (value === null)
    return (
      <Vital label="Block time" layout="mining">
        —
      </Vital>
    );
  const seconds = /^(\d+(?:\.\d+)?)s$/.exec(value);
  if (!seconds)
    return (
      <div role="alert" data-testid="vitals-block-time-error" className={STACKED}>
        <span className={LABEL}>Block time</span>
        <span className="text-rouge text-xs">{`block time ${JSON.stringify(value)} is not <seconds>s`}</span>
      </div>
    );
  return (
    <Vital label="Block time" layout="mining">
      {seconds[1]} s
    </Vital>
  );
}

function Vitals({ stats }: { stats: NetworkStats }) {
  return (
    <>
      <Vital label="Block" href={`/blocks/${stats.latestBlock}`} layout="block">
        {formatBlockNumber(stats.latestBlock)}
      </Vital>
      <EpochVital raw={stats.epoch} estimatedLeft={stats.estimatedEpochTime} />
      <BlockTime value={stats.avgBlockTime} />
      <Vital label="Hash rate" href="/charts/hash-rate" layout="mining">
        {stats.hashRate ?? '—'}
      </Vital>
      <Vital label="Difficulty" href="/charts/difficulty" layout="mining">
        {stats.difficulty}
      </Vital>
    </>
  );
}

function readEpoch(raw: string): { epoch: EpochInfo } | { error: string } {
  try {
    return { epoch: parseEpoch(raw) };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

/**
 * Epoch number, a neutral progress bar, `index / length` and the time left — or why it can't. Its
 * text never wraps: below md only the bar gives up width (down to 48 px), so the parts never stack
 * into a column.
 */
function EpochVital({ raw, estimatedLeft }: { raw: string; estimatedLeft: string | null }) {
  const read = readEpoch(raw);
  if ('error' in read) {
    return (
      <div
        role="alert"
        data-testid="vitals-epoch-error"
        className={`flex items-baseline gap-2 ${PLACE.line}`}
      >
        <span className={LABEL}>Epoch</span>
        <span className="text-rouge text-xs">{`unreadable: ${read.error}`}</span>
      </div>
    );
  }
  const { epoch } = read;
  const pct = (epoch.index / epoch.length) * 100;
  const index = epoch.index.toLocaleString('en-US');
  const length = epoch.length.toLocaleString('en-US');
  return (
    <Link
      href="/charts/epoch-time-length"
      data-testid="vitals-epoch"
      aria-label={`Epoch ${epoch.number.toLocaleString('en-US')}, block ${index} of ${length}, ${estimatedLeft === null ? 'remaining time unavailable' : `about ${estimatedLeft} left`}`}
      className={`group flex items-center gap-x-1.5 whitespace-nowrap md:gap-x-2 ${PLACE.line}`}
    >
      <span className={LABEL}>Epoch</span>
      <span className={VALUE}>{epoch.number.toLocaleString('en-US')}</span>
      <span
        aria-hidden="true"
        className="bg-base-border flex h-1 min-w-12 flex-[1_1_48px] overflow-hidden rounded-sm md:w-14 md:flex-none"
      >
        <span
          data-testid="vitals-epoch-progress"
          className="bg-text"
          style={{ width: `${pct}%` }}
        />
      </span>
      <span className="text-text text-meta tabular-nums md:text-xs">
        {index} / {length}
      </span>
      <span className={LABEL}>{estimatedLeft === null ? '—' : `≈ ${estimatedLeft} left`}</span>
    </Link>
  );
}

/**
 * A failed /statistics/network, said plainly: no stale or placeholder numbers. Like the homepage
 * figures, an error wins over data until a fetch succeeds again.
 */
function VitalsError({ message }: { message: string }) {
  return (
    <span role="alert" data-testid="vitals-error" className={`text-rouge text-xs ${PLACE.line}`}>
      {`Chain vitals unavailable: ${message}`}
    </span>
  );
}

/** The labels in the places their figures will take. */
const LOADING_SLOTS = [
  { label: 'Block', cls: `${INLINE} ${PLACE.block}` },
  { label: 'Epoch', cls: `${INLINE} ${PLACE.line}` },
  { label: 'Block time', cls: STACKED },
  { label: 'Hash rate', cls: STACKED },
  { label: 'Difficulty', cls: STACKED },
] as const;

function LoadingVital({ label, cls }: { label: string; cls: string }) {
  return (
    <div className={cls}>
      <span className={LABEL}>{label}</span>
      <span
        aria-hidden="true"
        className="bg-base-elevated inline-block h-3 w-16 animate-pulse rounded"
      />
    </div>
  );
}

function VitalsLoading() {
  return (
    <>
      {LOADING_SLOTS.map((slot) => (
        <LoadingVital key={slot.label} label={slot.label} cls={slot.cls} />
      ))}
    </>
  );
}

/**
 * Whether the page's realtime socket is up (Main.dc.html "● Live", TX-01): a jade dot and "Live"
 * when it is, a dim dot and "Not live" while it is not yet or no longer connected. Only a page
 * that holds the socket shows it.
 */
function LiveMark({ connected }: { connected: boolean }) {
  return (
    <span
      role="status"
      data-testid="vitals-live"
      data-live={connected}
      title={
        connected ? 'Live: realtime updates connected' : 'Not live: realtime updates not connected'
      }
      className={`text-small ml-auto flex items-center gap-2 ${PLACE.live} ${connected ? 'text-jade' : 'text-text-dim'}`}
    >
      <span
        aria-hidden="true"
        className={`${DOT_BOX} ${connected ? 'bg-jade' : 'bg-base-border'}`}
      />
      {connected ? 'Live' : 'Not live'}
    </span>
  );
}
