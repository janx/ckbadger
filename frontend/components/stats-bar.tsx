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
 * (Main.dc.html, TX-01). One row on a desktop; one row that scrolls inside itself on a phone
 * (TX-05). It replaces the homepage's old hero numbers, epoch card and floating LIVE pill.
 *
 * Every item is `shrink-0`: when the narrow row overflows, a shrinkable item would be squeezed to
 * its min-content width — and the epoch, the one item with several parts, would stack them into
 * a column while the rest stayed on the line. The row scrolls instead.
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
      className="flex min-w-0 flex-1 flex-nowrap items-baseline gap-x-4 gap-y-1 overflow-x-auto whitespace-nowrap py-2.5 font-mono md:flex-wrap md:gap-x-6 md:gap-y-2.5 md:overflow-visible md:whitespace-normal md:py-3"
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

const LABEL = 'text-text-dim text-[11px] md:text-xs';
const VALUE =
  'text-text-bright group-hover:text-jade text-[13px] font-medium tabular-nums transition-colors md:text-[15px]';

function Vital({ label, href, children }: { label: string; href?: string; children: ReactNode }) {
  const body = (
    <>
      <span className={LABEL}>{label}</span>
      <span className={VALUE}>{children}</span>
    </>
  );
  const cls = 'group flex shrink-0 items-baseline gap-1.5 md:gap-2';
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
  if (value === null) return <Vital label="Block time">—</Vital>;
  const seconds = /^(\d+(?:\.\d+)?)s$/.exec(value);
  if (!seconds)
    return (
      <div
        role="alert"
        data-testid="vitals-block-time-error"
        className="flex shrink-0 items-baseline gap-2"
      >
        <span className={LABEL}>Block time</span>
        <span className="text-rouge text-xs">{`block time ${JSON.stringify(value)} is not <seconds>s`}</span>
      </div>
    );
  return <Vital label="Block time">{seconds[1]} s</Vital>;
}

function Vitals({ stats }: { stats: NetworkStats }) {
  return (
    <>
      <Vital label="Block" href={`/blocks/${stats.latestBlock}`}>
        {formatBlockNumber(stats.latestBlock)}
      </Vital>
      <EpochVital raw={stats.epoch} estimatedLeft={stats.estimatedEpochTime} />
      <BlockTime value={stats.avgBlockTime} />
      <Vital label="Hash rate" href="/charts/hash-rate">
        {stats.hashRate ?? '—'}
      </Vital>
      <Vital label="Difficulty" href="/charts/difficulty">
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

/** Epoch number, a neutral progress bar, `index / length` and the time left — or why it can't. */
function EpochVital({ raw, estimatedLeft }: { raw: string; estimatedLeft: string | null }) {
  const read = readEpoch(raw);
  if ('error' in read) {
    return (
      <div
        role="alert"
        data-testid="vitals-epoch-error"
        className="flex shrink-0 items-baseline gap-2"
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
      className="group flex shrink-0 items-center gap-x-1.5 md:gap-x-2"
    >
      <span className={LABEL}>Epoch</span>
      <span className={VALUE}>{epoch.number.toLocaleString('en-US')}</span>
      <span
        aria-hidden="true"
        className="bg-base-border flex h-1 w-12 overflow-hidden rounded-sm md:w-14"
      >
        <span
          data-testid="vitals-epoch-progress"
          className="bg-text"
          style={{ width: `${pct}%` }}
        />
      </span>
      <span className="text-text text-[11px] tabular-nums md:text-xs">
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
    <span role="alert" data-testid="vitals-error" className="text-rouge text-xs">
      {`Chain vitals unavailable: ${message}`}
    </span>
  );
}

const LOADING_LABELS = ['Block', 'Epoch', 'Block time', 'Hash rate', 'Difficulty'];

function LoadingVital({ label }: { label: string }) {
  return (
    <div className="flex shrink-0 items-baseline gap-1.5 md:gap-2">
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
      {LOADING_LABELS.map((label) => (
        <LoadingVital key={label} label={label} />
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
      className={`ml-auto flex shrink-0 items-center gap-2 text-[12px] ${connected ? 'text-jade' : 'text-text-dim'}`}
    >
      <span
        aria-hidden="true"
        className={`${DOT_BOX} ${connected ? 'bg-jade' : 'bg-base-border'}`}
      />
      {connected ? 'Live' : 'Not live'}
    </span>
  );
}
