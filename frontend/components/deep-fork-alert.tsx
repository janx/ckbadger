'use client';

import Link from '@/components/ui/link';
import { DeepForkStatus } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';

interface DeepForkAlertProps {
  status: DeepForkStatus;
}

/** A detected deep fork always reports its heights; a missing one is a contract violation. */
function detectedHeight(value: number | null, field: string): string {
  if (value === null) throw new Error(`Deep fork detected but ${field} is missing`);
  return formatBlockNumber(value);
}

export function DeepForkAlert({ status }: DeepForkAlertProps) {
  if (!status.detected) return null;

  return (
    <div className="border-negative-bright bg-negative border-b text-white">
      <div className="container mx-auto px-4 py-4">
        <div className="flex flex-col items-center justify-between gap-4 md:flex-row">
          <div className="flex items-center gap-3">
            <div className="bg-negative-bright flex h-10 w-10 items-center justify-center rounded-full text-2xl">
              ⚠️
            </div>
            <div>
              <h2 className="text-lg font-bold">Chain Fork Detected - Sync Paused</h2>
              <p className="text-sm text-white/80">
                The local database is on a forked chain. Syncing has been paused to prevent data
                corruption.
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-6 text-sm">
            <div className="flex flex-col">
              <span className="text-xs uppercase text-white/70">Fork Depth</span>
              <span className="font-mono font-bold">{status.depth} blocks</span>
            </div>
            <div className="flex flex-col">
              <span className="text-xs uppercase text-white/70">DB Tip</span>
              <span className="font-mono font-bold">{detectedHeight(status.dbTip, 'dbTip')}</span>
            </div>
            <div className="flex flex-col">
              <span className="text-xs uppercase text-white/70">Chain Tip</span>
              <span className="font-mono font-bold">
                {detectedHeight(status.chainTip, 'chainTip')}
              </span>
            </div>
            <div className="flex flex-col">
              <span className="text-xs uppercase text-white/70">Fork Point</span>
              <span className="font-mono font-bold">
                {detectedHeight(status.forkPoint, 'forkPoint')}
              </span>
            </div>

            <Link
              href="/forks"
              className="bg-negative-bright hover:bg-negative-dim rounded-lg px-4 py-2 font-medium transition"
            >
              View Details →
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}
