'use client';

import Link from '@/components/ui/link';
import { DeepForkStatus } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { color, inkOnFill } from '@/lib/palette';
import { useTheme } from '@/hooks/useTheme';

interface DeepForkAlertProps {
  status: DeepForkStatus;
}

/** A detected deep fork always reports its heights; a missing one is a contract violation. */
function detectedHeight(value: number | null, field: string): string {
  if (value === null) throw new Error(`Deep fork detected but ${field} is missing`);
  return formatBlockNumber(value);
}

export function DeepForkAlert({ status }: DeepForkAlertProps) {
  const { resolved: theme } = useTheme();
  if (!status.detected) return null;
  // The banner is a fill: its words take the one ink the cinnabar's own luminance calls for, the
  // button's words the ink its brighter fill calls for (the fill-ink rule, inkOnFill).
  const ink = color(inkOnFill(theme, 'rouge', 1).ink);
  const buttonInk = color(inkOnFill(theme, 'rouge-bright', 1).ink);

  return (
    <div className="border-negative-bright bg-negative border-b" style={{ color: ink }}>
      <div className="container mx-auto py-4">
        <div className="flex flex-col items-center justify-between gap-4 md:flex-row">
          <div className="flex items-center gap-3">
            <div className="bg-negative-bright text-index flex h-10 w-10 items-center justify-center">
              ⚠️
            </div>
            <div>
              {/* The alert is not a section (rubbing × Song print, phase 5, H6): a plain bold h2
                  at the body step, so h2.text-section's cinnabar square and lowercase stay off
                  the red banner. */}
              <h2 className="text-body font-bold">Chain Fork Detected - Sync Paused</h2>
              <p className="text-body">
                The local database is on a forked chain. Syncing has been paused to prevent data
                corruption.
              </p>
            </div>
          </div>

          <div className="text-body flex flex-wrap items-center gap-6">
            <div className="flex flex-col">
              <span className="eyebrow text-meta">Fork Depth</span>
              <span className="font-mono font-bold">
                {status.depth === null
                  ? detectedHeight(status.depth, 'depth')
                  : formatCount(status.depth, 'block')}
              </span>
            </div>
            <div className="flex flex-col">
              <span className="eyebrow text-meta">DB Tip</span>
              <span className="font-mono font-bold">{detectedHeight(status.dbTip, 'dbTip')}</span>
            </div>
            <div className="flex flex-col">
              <span className="eyebrow text-meta">Chain Tip</span>
              <span className="font-mono font-bold">
                {detectedHeight(status.chainTip, 'chainTip')}
              </span>
            </div>
            <div className="flex flex-col">
              <span className="eyebrow text-meta">Fork Point</span>
              <span className="font-mono font-bold">
                {detectedHeight(status.forkPoint, 'forkPoint')}
              </span>
            </div>

            <Link
              href="/forks"
              className="bg-negative-bright hover:bg-negative-dim rounded-lg px-4 py-2 font-medium transition"
              style={{ color: buttonInk }}
            >
              View Details →
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}
