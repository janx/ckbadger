'use client';

import Link from '@/components/ui/link';
import { ReadoutRow } from '@/components/ui/readout-row';
import { usePinnedReadout, type PinnedReadout } from '@/hooks/usePinnedReadout';
import { formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import type { HardforkScriptDeployment } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';

interface Props {
  deployments: HardforkScriptDeployment[];
  /**
   * The time axis the marks sit on, in ms: the lane timeline's genesis → tip. Omitted, the strip
   * spans the deployments themselves.
   */
  domain?: readonly [number, number];
  /** Pixel insets matching the lane timeline's label and live-reference columns. */
  inset?: { left: number; right: number };
  /** The label beside (or above) the strip; omitted, the strip counts its versions. */
  label?: string;
  /**
   * The hover/tap state over version hashes, shared with the lane dots; omitted, the strip keeps
   * its own (sticky, see `deploymentReadout`).
   */
  readout?: PinnedReadout<string>;
  /** The lane timeline draws one evidence panel for lanes and strip alike. */
  hideEvidence?: boolean;
}

/** Columns a mark can take on the strip; marks in the same column stack downward. */
const COLUMNS = 200;
const ROW_PX = 8;

function headerMs(timestampMs: number): number {
  if (
    !Number.isSafeInteger(timestampMs) ||
    timestampMs < 0 ||
    timestampMs > 8_640_000_000_000_000
  ) {
    throw new Error(`Invalid indexed script deployment timestamp: ${timestampMs}`);
  }
  return timestampMs;
}

/** A version's name is not unique (two "PW Lock" versions): the version hash tells them apart. */
export function markName(mark: HardforkScriptDeployment): string {
  return `${mark.versionName} · ${mark.familyName} · version ${mark.versionHash.slice(0, 10)}…`;
}

/**
 * The deployment dots' readout (R6): a hovered or focused dot names its version and keeps it until
 * another is hovered, so the pointer can reach the evidence links; a tap pins it until a second
 * tap on it or Escape.
 */
export function useDeploymentReadout(): PinnedReadout<string> {
  return usePinnedReadout<string>(String, { sticky: true });
}

/**
 * Each known script version's earliest indexed code cell as a dot on the time axis (CH-40): no
 * scrolling list, every mark in place; hover, tap or focus a dot for its evidence (CH-44).
 */
export function ScriptDeploymentTimeline({
  deployments,
  domain,
  inset,
  label,
  readout: shared,
  hideEvidence = false,
}: Props) {
  const own = useDeploymentReadout();
  const readout = shared ?? own;
  const selectedHash = readout.active;
  const marks = [...deployments].sort(
    (left, right) =>
      left.blockNumber - right.blockNumber ||
      left.txIndex - right.txIndex ||
      left.codeCellOutputIndex - right.codeCellOutputIndex ||
      left.versionHash.localeCompare(right.versionHash)
  );
  const selected = marks.find((mark) => mark.versionHash === selectedHash) ?? null;

  if (marks.length === 0) {
    return (
      <p className="text-text-dim text-small">
        No named script-version code-cell deployments are indexed yet.
      </p>
    );
  }

  const times = marks.map((mark) => headerMs(mark.timestampMs));
  const [start, end] = domain ?? [Math.min(...times) - 86_400_000, Math.max(...times) + 86_400_000];
  if (!(start < end)) throw new Error(`Invalid deployment time axis ${start}..${end}`);
  const filled = new Map<number, number>();
  const placed = marks.map((mark, i) => {
    const fraction = (times[i] - start) / (end - start);
    if (fraction < 0 || fraction > 1)
      throw new Error(
        `Script deployment ${mark.versionHash} at ${formatUtc8DateTime(times[i])} lies outside the indexed chain time`
      );
    const column = Math.min(COLUMNS - 1, Math.floor(fraction * COLUMNS));
    const row = filled.get(column) ?? 0;
    filled.set(column, row + 1);
    return { mark, left: (column + 0.5) / COLUMNS, row };
  });
  const rows = Math.max(...placed.map((entry) => entry.row)) + 1;
  const left = inset?.left ?? 0;
  const right = inset?.right ?? 0;
  const caption = label ?? `known script versions · ${marks.length.toLocaleString()}`;
  // Too little room beside the strip (a phone): the label goes above it.
  const labelAbove = left < 80;

  return (
    <div className="min-w-0">
      {labelAbove && (
        <span className="text-text-dim text-meta mb-1 block" style={{ paddingLeft: left }}>
          {caption}
        </span>
      )}
      <div className="relative" style={{ height: rows * ROW_PX + 6 }}>
        {!labelAbove && (
          <span
            className="text-text-dim text-meta absolute left-0 top-0 leading-tight"
            style={{ width: left - 8 }}
          >
            {caption}
          </span>
        )}
        <div
          role="list"
          aria-label="Known script-version deployment timeline"
          className="absolute inset-y-0"
          style={{ left, right }}
        >
          {placed.map(({ mark, left: x, row }) => {
            const active = mark.versionHash === selected?.versionHash;
            return (
              <span
                role="listitem"
                key={mark.versionHash}
                className="absolute"
                style={{ left: `${x * 100}%`, top: row * ROW_PX }}
              >
                <button
                  type="button"
                  title={`${mark.versionName} · deployed ${formatUtc8Date(mark.timestampMs)}`}
                  aria-label={`${markName(mark)}, first code cell at block ${formatBlockNumber(mark.blockNumber)}, ${formatUtc8Date(mark.timestampMs)}`}
                  {...readout.bind(mark.versionHash)}
                  className={`focus-visible:outline-jade block h-[7px] w-[7px] -translate-x-1/2 rounded-full border focus-visible:outline ${active ? 'bg-jade border-jade' : 'border-text-bright bg-base-bg hover:bg-jade'}`}
                />
              </span>
            );
          })}
        </div>
      </div>
      {!hideEvidence && (
        <DeploymentEvidence
          selected={selected}
          pinned={selected !== null && readout.isPinned(selected.versionHash)}
        />
      )}
    </div>
  );
}

/**
 * One deployment's evidence under the timeline: its readout line (name and day, "tapped" when
 * pinned, R6), then what the mark means, header time, epoch, version and links.
 */
export function DeploymentEvidence({
  selected,
  pinned,
}: {
  selected: HardforkScriptDeployment | null;
  pinned: boolean;
}) {
  return (
    <aside aria-label="Selected script deployment evidence" className="text-small mt-3 min-w-0">
      {/* The row's own "tap or hover" is the section's one hint (R14); the idle line says what a
          dot reads. */}
      <ReadoutRow
        label="Script version readout"
        pinned={pinned}
        idle="a dot reads its script version, its first code cell and its block"
      >
        {selected
          ? `${markName(selected)} · deployed ${formatUtc8Date(selected.timestampMs)}`
          : null}
      </ReadoutRow>
      {selected && (
        <>
          <p className="text-text-dim mt-2 max-w-[70ch] leading-relaxed">
            Earliest indexed code-cell creation for this named version. This is a code-cell event,
            not proof that a script instance used a particular VM or that the code remains live.
          </p>
          <dl className="mt-3 grid grid-cols-[5rem_minmax(0,1fr)] gap-y-2">
            <dt className="text-text-dim">Header time</dt>
            <dd className="break-words">{formatUtc8DateTime(selected.timestampMs)}</dd>
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
                Block {formatBlockNumber(selected.blockNumber)}
              </Link>
            </dd>
          </dl>
        </>
      )}
    </aside>
  );
}
