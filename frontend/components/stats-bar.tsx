'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import { api, type NetworkStats } from '@/lib/api';
import { parseEpoch, type EpochInfo } from '@/lib/epoch';
import { formatBlockNumber } from '@/lib/format-block';

/**
 * The chain vitals strip under the header, on every page: whether realtime updates are live (a dot
 * before the block), block, epoch progress, block time, hash rate and difficulty — one row on a
 * desktop. It replaces the homepage's old hero numbers, epoch card and floating LIVE pill.
 */
export function VitalsStrip() {
  const { data: stats, error } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });
  const isConnected = useRealtimeStore((s) => s.isConnected);

  return (
    <section
      aria-label="Chain vitals"
      data-testid="vitals-strip"
      className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-4 gap-y-1 py-2.5 font-mono md:gap-x-6 md:gap-y-2.5 md:py-3"
    >
      {error ? (
        <VitalsError connected={isConnected} message={error.message} />
      ) : stats ? (
        <Vitals stats={stats} connected={isConnected} />
      ) : (
        <VitalsLoading connected={isConnected} />
      )}
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
  const cls = 'group flex items-baseline gap-1.5 md:gap-2';
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
 * The realtime dot and the item it leads, kept together so the dot never wraps alone. A page that
 * does not hold the realtime socket gets an empty box of the same size instead: no "Not live" for
 * a connection it never makes, and the strip lines up the same on every page.
 */
function Lead({ connected, children }: { connected: boolean; children: ReactNode }) {
  const held = useRealtimeStore((s) => s.socketHeld);
  return (
    <div className="flex items-baseline gap-2">
      {held ? (
        <LiveDot connected={connected} />
      ) : (
        <span aria-hidden="true" data-testid="vitals-live-spacer" className={DOT_BOX} />
      )}
      {children}
    </div>
  );
}

function Vitals({ stats, connected }: { stats: NetworkStats; connected: boolean }) {
  return (
    <>
      <Lead connected={connected}>
        <Vital label="Block" href={`/blocks/${stats.latestBlock}`}>
          {formatBlockNumber(stats.latestBlock)}
        </Vital>
      </Lead>
      <EpochVital raw={stats.epoch} estimatedLeft={stats.estimatedEpochTime} />
      <Vital label="Block time">{stats.avgBlockTime ?? '—'}</Vital>
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
      <div role="alert" data-testid="vitals-epoch-error" className="flex items-baseline gap-2">
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
      className="group flex flex-wrap items-center gap-x-1.5 gap-y-1 md:gap-x-2"
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
function VitalsError({ connected, message }: { connected: boolean; message: string }) {
  return (
    <Lead connected={connected}>
      <span role="alert" data-testid="vitals-error" className="text-rouge text-xs">
        {`Chain vitals unavailable: ${message}`}
      </span>
    </Lead>
  );
}

const LOADING_LABELS = ['Block', 'Epoch', 'Block time', 'Hash rate', 'Difficulty'];

function LoadingVital({ label }: { label: string }) {
  return (
    <div className="flex items-baseline gap-1.5 md:gap-2">
      <span className={LABEL}>{label}</span>
      <span
        aria-hidden="true"
        className="bg-base-elevated inline-block h-3 w-16 animate-pulse rounded"
      />
    </div>
  );
}

function VitalsLoading({ connected }: { connected: boolean }) {
  const [first, ...rest] = LOADING_LABELS;
  return (
    <>
      <Lead connected={connected}>
        <LoadingVital label={first} />
      </Lead>
      {rest.map((label) => (
        <LoadingVital key={label} label={label} />
      ))}
    </>
  );
}

/**
 * Whether the page's realtime socket is up: a jade dot when it is, a dim one while it is not yet or
 * no longer connected. The word is its accessible name and tooltip.
 */
function LiveDot({ connected }: { connected: boolean }) {
  return (
    <span
      role="status"
      data-testid="vitals-live"
      data-live={connected}
      aria-label={connected ? 'Live' : 'Not live'}
      title={
        connected ? 'Live: realtime updates connected' : 'Not live: realtime updates not connected'
      }
      className={`${DOT_BOX} ${connected ? 'bg-jade' : 'bg-base-border'}`}
    />
  );
}
