'use client';

import Link from '@/components/ui/link';
import { DeepForkStatus } from '@/lib/api';

interface DeepForkAlertProps {
  status: DeepForkStatus;
}

export function DeepForkAlert({ status }: DeepForkAlertProps) {
  if (!status.detected) return null;

  return (
    <div className="border-b border-red-700 bg-red-900/90 text-white">
      <div className="container mx-auto px-4 py-4">
        <div className="flex flex-col items-center justify-between gap-4 md:flex-row">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-full bg-red-800 text-2xl">
              ⚠️
            </div>
            <div>
              <h2 className="text-lg font-bold">Chain Fork Detected - Sync Paused</h2>
              <p className="text-sm text-red-200">
                The local database is on a forked chain. Syncing has been paused to prevent data
                corruption.
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-6 text-sm">
            <div className="flex flex-col">
              <span className="text-xs uppercase text-red-300">Fork Depth</span>
              <span className="font-mono font-bold">{status.depth} blocks</span>
            </div>
            <div className="flex flex-col">
              <span className="text-xs uppercase text-red-300">DB Tip</span>
              <span className="font-mono font-bold">#{status.dbTip?.toLocaleString()}</span>
            </div>
            <div className="flex flex-col">
              <span className="text-xs uppercase text-red-300">Chain Tip</span>
              <span className="font-mono font-bold">#{status.chainTip?.toLocaleString()}</span>
            </div>
            <div className="flex flex-col">
              <span className="text-xs uppercase text-red-300">Fork Point</span>
              <span className="font-mono font-bold">#{status.forkPoint?.toLocaleString()}</span>
            </div>

            <Link
              href="/forks"
              className="rounded-lg bg-red-800 px-4 py-2 font-medium transition hover:bg-red-700"
            >
              View Details →
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}
