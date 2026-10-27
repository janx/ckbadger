'use client';

import { useState } from 'react';
import Link from '@/components/ui/link';
import type { HardforkScriptDeployment } from '@/lib/api';

interface Props {
  deployments: HardforkScriptDeployment[];
}

function utcTimestamp(timestampMs: number): string {
  if (
    !Number.isSafeInteger(timestampMs) ||
    timestampMs < 0 ||
    timestampMs > 8_640_000_000_000_000
  ) {
    throw new Error(`Invalid indexed script deployment timestamp: ${timestampMs}`);
  }
  return new Date(timestampMs).toISOString();
}

export function ScriptDeploymentTimeline({ deployments }: Props) {
  const [selectedHash, setSelectedHash] = useState<string | null>(null);
  const marks = [...deployments].sort(
    (left, right) =>
      left.blockNumber - right.blockNumber ||
      left.txIndex - right.txIndex ||
      left.codeCellOutputIndex - right.codeCellOutputIndex ||
      left.versionHash.localeCompare(right.versionHash)
  );
  const selected = marks.find((mark) => mark.versionHash === selectedHash) ?? marks[0];

  if (marks.length === 0) {
    return (
      <p className="text-text-dim text-[12px]">
        No named script-version code-cell deployments are indexed yet.
      </p>
    );
  }

  return (
    <div className="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <ol
        aria-label="Known script-version deployment timeline"
        className="border-base-border-subtle max-h-80 min-w-0 overflow-y-auto border-l pl-4"
      >
        {marks.map((mark) => (
          <li key={mark.versionHash} className="relative min-w-0 py-2">
            <span
              aria-hidden="true"
              className="bg-text-bright absolute -left-[21px] top-4 h-[9px] w-[9px] rounded-full"
            />
            <button
              type="button"
              className="text-text-bright hover:text-jade focus-visible:text-jade min-w-0 text-left text-[12px] focus-visible:outline"
              onMouseEnter={() => setSelectedHash(mark.versionHash)}
              onFocus={() => setSelectedHash(mark.versionHash)}
              onClick={() => setSelectedHash(mark.versionHash)}
              aria-label={`${mark.versionName}, first code cell at block #${mark.blockNumber}`}
            >
              <span className="text-text-dim mr-3 tabular-nums">
                {utcTimestamp(mark.timestampMs).slice(0, 10)}
              </span>
              <span className="break-all">{mark.versionName}</span>
            </button>
          </li>
        ))}
      </ol>
      <aside
        aria-label="Selected script deployment evidence"
        className="border-base-border-subtle min-w-0 border-t pt-3 text-[12px]"
      >
        <p className="text-text-bright break-words">{selected.versionName}</p>
        <p className="text-text-dim mt-2 leading-relaxed">
          Earliest indexed code-cell creation for this named version. This is a code-cell event, not
          proof that a script instance used a particular VM or that the code remains live.
        </p>
        <dl className="mt-3 grid grid-cols-[5rem_minmax(0,1fr)] gap-y-2">
          <dt className="text-text-dim">Header time</dt>
          <dd className="break-words">{utcTimestamp(selected.timestampMs)}</dd>
          <dt className="text-text-dim">Epoch</dt>
          <dd>{selected.epochNumber.toLocaleString()}</dd>
          <dt className="text-text-dim">Version</dt>
          <dd className="break-all font-mono">{selected.versionHash}</dd>
          <dt className="text-text-dim">Evidence</dt>
          <dd className="flex min-w-0 flex-wrap gap-x-3 gap-y-1">
            <Link
              href={`/scripts/${encodeURIComponent(selected.familyName)}`}
              className="text-jade hover:underline"
            >
              {selected.familyName} family
            </Link>
            <Link
              href={`/cell/${selected.codeCellTxHash}-${selected.codeCellOutputIndex}`}
              className="text-jade hover:underline"
            >
              Code cell {selected.codeCellTxHash.slice(0, 10)}…:{selected.codeCellOutputIndex}
            </Link>
            <Link href={`/blocks/${selected.blockNumber}`} className="text-jade hover:underline">
              Block #{selected.blockNumber.toLocaleString()}
            </Link>
          </dd>
        </dl>
      </aside>
    </div>
  );
}
