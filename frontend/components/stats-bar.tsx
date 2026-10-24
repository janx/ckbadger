'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import { api, type NetworkStats } from '@/lib/api';
import { parseEpoch, type EpochInfo } from '@/lib/epoch';

/**
 * The chain vitals strip under the header, on every page: block, epoch progress, block time,
 * hash rate, difficulty, and whether realtime updates are live. It replaces the homepage's old hero
 * numbers, epoch card and floating LIVE pill.
 */
export function VitalsStrip() {
  const { data: stats } = useQuery({
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
      className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-4 gap-y-1 py-2.5 md:gap-x-9 md:gap-y-2.5 md:py-3"
    >
      {stats ? <Vitals stats={stats} /> : <VitalsLoading />}
      {isConnected && <LiveStatus />}
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

function Vitals({ stats }: { stats: NetworkStats }) {
  return (
    <>
      <Vital label="Block" href={`/blocks/${stats.latestBlock}`}>
        #{stats.latestBlock.toLocaleString('en-US')}
      </Vital>
      <EpochVital raw={stats.epoch} estimatedLeft={stats.estimatedEpochTime} />
      <Vital label="Block time">{stats.avgBlockTime}</Vital>
      <Vital label="Hash rate" href="/charts/hash-rate">
        {stats.hashRate}
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
function EpochVital({ raw, estimatedLeft }: { raw: string; estimatedLeft: string }) {
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
      aria-label={`Epoch ${epoch.number.toLocaleString('en-US')}, block ${index} of ${length}, about ${estimatedLeft} left`}
      className="group flex flex-wrap items-center gap-x-1.5 gap-y-1 md:gap-x-2"
    >
      <span className={LABEL}>Epoch</span>
      <span className={VALUE}>{epoch.number.toLocaleString('en-US')}</span>
      <span
        aria-hidden="true"
        className="bg-base-border flex h-1 w-12 overflow-hidden rounded-sm md:w-[72px]"
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
      <span className={LABEL}>≈ {estimatedLeft} left</span>
    </Link>
  );
}

function VitalsLoading() {
  return (
    <>
      {['Block', 'Epoch', 'Block time', 'Hash rate', 'Difficulty'].map((label) => (
        <div key={label} className="flex items-baseline gap-1.5 md:gap-2">
          <span className={LABEL}>{label}</span>
          <span
            aria-hidden="true"
            className="bg-base-elevated inline-block h-3 w-16 animate-pulse rounded"
          />
        </div>
      ))}
    </>
  );
}

/** Shown while the realtime WebSocket is connected. */
function LiveStatus() {
  return (
    <div role="status" data-testid="vitals-live" className="ml-auto flex items-center gap-2">
      <span aria-hidden="true" className="bg-jade h-[7px] w-[7px] rounded-full" />
      <span className="text-jade text-[11px] md:text-xs">Live</span>
    </div>
  );
}
