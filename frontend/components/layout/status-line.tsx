'use client';

import { useSyncExternalStore, type ReactNode } from 'react';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { useElementWidth } from '@/hooks/useElementWidth';
import { useFromMd } from '@/hooks/useFromMd';
import { useNetworkStats } from '@/hooks/useNetworkStats';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import type { NetworkStats } from '@/lib/api';
import { chainMode, type ChainMode } from '@/lib/chain-mode';
import { clockNow, clockText, subscribeClock, zoned } from '@/lib/clock';
import { formatBlockNumber } from '@/lib/format-block';
import { releaseOf, resolveBuildVersion } from '@/lib/runtime-config';
import { STATUS_KEY_HINTS } from '@/lib/shortcuts';
import { TIP_WORD, fitStatusLine } from '@/lib/status-line-fit';
import { cn } from '@/lib/utils';

/** The mode block's ground per mode: celadon live, gold (the warning hue) syncing, cinnabar off. */
const MODE_CLASS: Record<ChainMode, string> = {
  LIVE: 'bg-jade text-base-bg',
  SYNCING: 'bg-gold text-base-bg',
  OFFLINE: 'bg-rouge text-base-bg',
};

/** A segment: its own cell between 1 px lines, the line on the side away from the edge it keeps. */
function Segment({
  children,
  side = 'left',
  className,
  title,
}: {
  children: ReactNode;
  side?: 'left' | 'right';
  className?: string;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        'border-base-border flex items-center gap-1.5 px-3',
        side === 'left' ? 'border-r' : 'border-l',
        className
      )}
    >
      {children}
    </span>
  );
}

/**
 * The status line (rubbing × Song print, phase 2): the page is a terminal session and this is its
 * last row, fixed at the foot of the window, 26 px on the elevated ground under a 1 px line. From
 * the left: the mode — `LIVE` on a celadon block, `SYNCING` gold, `OFFLINE` cinnabar, from the
 * vitals strip's own query and Live mark (lib/chain-mode) — the network, the tip, then `ckbadger
 * <release> · synced`; at the right the key hints (the help's own list, lib/shortcuts) and a local
 * clock. The API reports no node version, so none is shown.
 *
 * The narrow form (below md) is one row: the mode, the network, the tip and the clock. The release
 * and the key hints are the wide form's only: a touch screen has no keys. From md the release is
 * here and the clock's zone in the `?` help; below md, where nothing opens the help (the footer's
 * "Keyboard shortcuts" is drawn from md only, PA-29), the footer's last row says both
 * (components/layout/site-footer.tsx) — designed, not dropped. The form follows md (useFromMd, the
 * footer's own breakpoint for that row).
 *
 * Within its form the line draws a part only where its width holds it (readability F3-f): it
 * measures itself before paint and gives its parts up in one order, the key hints from the last,
 * then the tip's word, the tip, the network, the release; the mode and the clock never
 * (lib/status-line-fit.ts, the numbers there). Never two rows, never a part cut at the edge.
 *
 * It is a polite live region for the mode and the release; the tip (every block) and the clock
 * (every second) are `aria-live="off"`, so a screen reader is not read the time.
 */
export function StatusLine() {
  const { data: stats, error } = useNetworkStats();
  const socketHeld = useRealtimeStore((s) => s.socketHeld);
  const connected = useRealtimeStore((s) => s.isConnected);
  const network = useActiveNetwork();
  const buildVersion = resolveBuildVersion();
  const fromMd = useFromMd();
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const mode = chainMode({ stats, failed: error !== null, socketHeld, connected });
  const sync = stats === undefined ? null : stats.syncStatus.isSyncing ? 'syncing' : 'synced';
  const tip = stats === undefined ? '—' : formatBlockNumber(stats.latestBlock);
  const release = `ckbadger ${releaseOf(buildVersion)}${sync === null ? '' : ` · ${sync}`}`;
  const parts = fitStatusLine(
    fromMd ? 'wide' : 'narrow',
    { mode: mode ?? '—', network, tip, release, hints: STATUS_KEY_HINTS },
    width
  );

  return (
    <div
      ref={ref}
      role="status"
      aria-label="Status line"
      data-testid="status-line"
      data-measured={width !== null}
      className="bg-base-elevated border-base-border text-meta text-text-dim fixed inset-x-0 bottom-0 z-40 flex h-[26px] items-stretch overflow-hidden whitespace-nowrap border-t font-mono"
    >
      <span
        data-testid="status-mode"
        data-mode={mode ?? 'unknown'}
        title={modeTitle(mode, error, stats)}
        className={cn(
          'flex items-center px-2.5 font-semibold',
          mode === null ? 'bg-base-border text-text-dim' : MODE_CLASS[mode]
        )}
      >
        {mode ?? '—'}
      </span>
      {parts.network && <Segment>{network}</Segment>}
      {parts.tip !== null && (
        <Segment>
          {/* the number alone where the line cannot hold the word: its `#` says it is a block */}
          {parts.tip === 'labelled' ? TIP_WORD : <span className="sr-only">{`${TIP_WORD} `}</span>}
          <span aria-live="off" className="text-text-bright tabular-nums">
            {tip}
          </span>
        </Segment>
      )}
      {parts.release && <Segment title={`ckbadger ${buildVersion}`}>{release}</Segment>}
      <span className="ml-auto flex">
        {STATUS_KEY_HINTS.slice(0, parts.hints).map((hint) => (
          <Segment key={hint.keys} side="right">
            <kbd className="text-text-bright font-mono">{hint.keys}</kbd>
            {hint.short}
          </Segment>
        ))}
        <Segment side="right">
          <StatusClock />
        </Segment>
      </span>
    </div>
  );
}

/** What the mode block means, on hover. */
function modeTitle(
  mode: ChainMode | null,
  error: Error | null,
  stats: NetworkStats | undefined
): string {
  if (mode === null) return 'Waiting for the chain vitals';
  if (mode === 'OFFLINE')
    return error
      ? `Offline: chain vitals unavailable: ${error.message}`
      : 'Offline: realtime updates not connected';
  if (mode === 'LIVE') return 'Live: the explorer follows the chain tip';
  if (stats === undefined)
    throw new Error('status line: SYNCING without the vitals it was read from');
  const { syncedBlock, tipBlock } = stats.syncStatus;
  return `Syncing: indexed ${formatBlockNumber(syncedBlock)} of ${formatBlockNumber(tipBlock)}`;
}

/**
 * The clock: local time, ticking on the second, read from the page's one clock (lib/clock.ts, the
 * same the ages read; each tick waits for the next whole second).
 */
export function StatusClock() {
  const now = new Date(useSyncExternalStore(subscribeClock, clockNow, clockNow));
  return (
    <time
      aria-live="off"
      data-testid="status-clock"
      dateTime={now.toISOString()}
      title={zoned('Local time')}
      className="tabular-nums"
    >
      {clockText(now)}
    </time>
  );
}
