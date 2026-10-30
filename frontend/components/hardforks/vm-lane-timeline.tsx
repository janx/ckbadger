'use client';

import { useState } from 'react';
import { useElementWidth } from '@/hooks/useElementWidth';
import {
  DeploymentEvidence,
  ScriptDeploymentTimeline,
  markName,
} from '@/components/hardforks/script-deployment-timeline';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatExactDecimal } from '@/lib/exact-integer';
import { formatUtc8Date } from '@/lib/utils';
import type { HardforkHashTypeLaneF2, HardforkScriptDeployment } from '@/lib/api';

export interface EditionMark {
  shortName: string;
  activationEpoch: number;
  /** The UTC+8 day the edition's activation epoch started. */
  activationDate: string;
}

interface Props {
  /** The two editions in activation order: the one that added VM1, then the one that added VM2. */
  editions: readonly [EditionMark, EditionMark];
  lanes: readonly HardforkHashTypeLaneF2[];
  deployments: HardforkScriptDeployment[];
  genesisMs: number;
  tipMs: number;
  tipEpoch: number;
}

/** The lanes in VM order, as the API sends them (R11). */
export const LANE_ORDER = ['type', 'data1', 'data2', 'data'] as const;
type Lane = (typeof LANE_ORDER)[number];
const LANE_TEXT: Record<Lane, string> = {
  type: 'the newest VM',
  data1: 'pinned to VM1',
  data2: 'pinned to VM2',
  data: 'pinned to VM0',
};
/** The VMs are one hue, deeper for newer: the board's encoding. */
const VM_OPACITY = { VM0: 0.16, VM1: 0.32, VM2: 0.55 } as const;
const UTC8_MS = 8 * 3_600_000;
/** Average glyph width of the 10–11 px mono labels, for keeping labels apart. */
const CHAR_PX = 6.5;

/** An edition's activation day as an instant on the axis: that UTC+8 day's start. */
export function dayStartMs(day: string): number {
  const ms = Date.parse(`${day}T00:00:00+08:00`);
  if (!Number.isFinite(ms)) throw new Error(`Invalid hardfork activation date ${day}`);
  return ms;
}

/** Two rows of dots under a lane bar: a dot takes the first row where it does not touch another. */
function laneDotRows(xs: readonly number[]): number[] {
  const last = [-Infinity, -Infinity];
  return xs.map((x) => {
    const row = x - last[0] >= 7 ? 0 : x - last[1] >= 7 ? 1 : last[0] <= last[1] ? 0 : 1;
    last[row] = x;
    return row;
  });
}

/**
 * Which VM runs a script, over time (CH-40): genesis to now with the two edition lines, one lane
 * per hash type with its VM segments, each registered script version as a dot on the lane of every
 * hash type it is registered under (R2b-b), and each lane's live references from R11 (CH-41).
 * Versions with no registered reference sit on a strip below. Lanes follow the protocol rule —
 * `type` follows the newest VM, each data hash type pins one — and nothing more. Below 640 px the
 * labels move above each lane so the time axis keeps the full width.
 */
export function VmLaneTimeline({
  editions,
  lanes,
  deployments,
  genesisMs,
  tipMs,
  tipEpoch,
}: Props) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const [selectedHash, setSelectedHash] = useState<string | null>(null);
  if (!(genesisMs < tipMs)) throw new Error('VM lane timeline needs a tip after genesis');
  if (lanes.map((lane) => lane.hashType).join() !== LANE_ORDER.join())
    throw new Error(`hash-type lanes out of VM order: ${lanes.map((l) => l.hashType).join()}`);
  for (const deployment of deployments)
    if (!deployment.hashTypes)
      throw new Error(`script deployment ${deployment.versionHash} has no registered hash types`);
  const [vm1, vm2] = editions;
  const t1 = dayStartMs(vm1.activationDate);
  const t2 = dayStartMs(vm2.activationDate);
  if (!(genesisMs < t1 && t1 < t2 && t2 <= tipMs))
    throw new Error('hardfork activations must lie in order between genesis and the tip');
  const narrow = width !== null && width < 640;
  const left = narrow ? 8 : 190;
  const right = narrow ? 8 : 150;
  const top = narrow ? 62 : 64;
  const laneH = narrow ? 64 : 46;
  /** Where a lane's bar and its dot rows sit, from the lane's top. */
  const bar = narrow ? { y: 18, h: 14, dots: [40, 48] } : { y: 6, h: 16, dots: [30, 38] };
  const bottom = top + laneH * LANE_ORDER.length;
  const height = bottom + 44;
  const w = width ?? 0;
  const x = (t: number) => left + ((t - genesisMs) / (tipMs - genesisMs)) * (w - left - right);
  const firstYear = new Date(genesisMs + UTC8_MS).getUTCFullYear() + 1;
  const lastYear = new Date(tipMs + UTC8_MS).getUTCFullYear();
  const allYears = Array.from(
    { length: Math.max(0, lastYear - firstYear + 1) },
    (_, i) => firstYear + i
  );
  const yearX = (year: number) => x(Date.UTC(year, 0, 1) - UTC8_MS);
  // Every year while labels fit, else every other one.
  const yearStep = allYears.length > 1 && yearX(allYears[1]) - yearX(allYears[0]) < 34 ? 2 : 1;
  const years = allYears.filter((_, i) => i % yearStep === 0);
  const segments: Record<Lane, [number, number, keyof typeof VM_OPACITY][]> = {
    type: [
      [genesisMs, t1, 'VM0'],
      [t1, t2, 'VM1'],
      [t2, tipMs, 'VM2'],
    ],
    data1: [[t1, tipMs, 'VM1']],
    data2: [[t2, tipMs, 'VM2']],
    data: [[genesisMs, tipMs, 'VM0']],
  };
  // Edition labels: a label that would run off the right edge turns left of its line, and a
  // sub-label that would touch the one before it drops a row (390 px: the two epochs collided).
  const editionLabels = editions.reduce<
    {
      edition: EditionMark;
      at: number;
      sub: string;
      turn: boolean;
      subY: number;
      x0: number;
      x1: number;
    }[]
  >((placed, edition) => {
    const at = x(dayStartMs(edition.activationDate));
    const epoch = `epoch ${formatExactDecimal(String(edition.activationEpoch), 'edition epoch')}`;
    const sub = narrow ? epoch : `${epoch} · ${edition.activationDate}`;
    const span = sub.length * CHAR_PX;
    const turn = at + 6 + span > w;
    const x0 = turn ? at - 6 - span : at + 6;
    const before = placed[placed.length - 1];
    const touches =
      before !== undefined && before.subY === 33 && x0 < before.x1 + 4 && before.x0 < x0 + span + 4;
    return [...placed, { edition, at, sub, turn, subY: touches ? 46 : 33, x0, x1: x0 + span }];
  }, []);
  const byTime = [...deployments].sort(
    (a, b) => a.timestampMs - b.timestampMs || a.versionHash.localeCompare(b.versionHash)
  );
  const unregistered = byTime.filter((deployment) => deployment.hashTypes!.length === 0);
  const selected = deployments.find((d) => d.versionHash === selectedHash) ?? null;
  return (
    <div ref={ref} className="min-w-0">
      {width !== null && width > left + right && (
        <svg
          role="img"
          aria-label="Which VM each hash type runs, from genesis to now: the two editions, each hash type's VM, and live references per hash type"
          width={width}
          height={height}
          viewBox={`0 0 ${width} ${height}`}
          className="block"
        >
          {years.map((year) => {
            const at = yearX(year);
            return (
              <g key={year}>
                <line x1={at} x2={at} y1={top - 8} y2={bottom} stroke="#141826" />
                <text data-year x={at + 3} y={bottom + 16} fontSize="11" fill="#7c849c">
                  {year}
                </text>
              </g>
            );
          })}
          <text x={left} y="18" fontSize="12.5" fill="#dee2ec">
            genesis
          </text>
          <text x={left} y="33" fontSize="10.5" fill="#7c849c">
            {formatUtc8Date(genesisMs)}
          </text>
          {editionLabels.map(({ edition, at, sub, turn, subY }) => {
            return (
              <g key={edition.shortName}>
                <line
                  data-edition={edition.shortName}
                  x1={at}
                  x2={at}
                  y1="10"
                  y2={bottom}
                  stroke="#dee2ec"
                  strokeWidth="1.2"
                />
                <text
                  x={turn ? at - 6 : at + 6}
                  y="18"
                  textAnchor={turn ? 'end' : 'start'}
                  fontSize="12.5"
                  fill="#dee2ec"
                >
                  {edition.shortName}
                </text>
                <text
                  x={turn ? at - 6 : at + 6}
                  y={subY}
                  textAnchor={turn ? 'end' : 'start'}
                  fontSize="10.5"
                  fill="#7c849c"
                >
                  {sub}
                </text>
              </g>
            );
          })}
          {/* On a phone each lane's header carries its own count. */}
          {!narrow && (
            <text x={width} y={top - 12} textAnchor="end" fontSize="10.5" fill="#7c849c">
              live references
            </text>
          )}
          {LANE_ORDER.map((hashType, i) => {
            const y = top + i * laneH;
            const lane = lanes[i];
            const pinnedFrom = hashType === 'data1' ? t1 : hashType === 'data2' ? t2 : null;
            const pinnedBy = hashType === 'data1' ? vm1.shortName : vm2.shortName;
            // data2's note sits between the editions, clear of the Mirana line.
            const noteFrom = hashType === 'data2' ? t1 : genesisMs;
            const live = formatExactDecimal(lane.liveReferences, `${hashType} live references`);
            const versions = `${lane.knownVersions.toLocaleString()} known version${lane.knownVersions === 1 ? '' : 's'}`;
            const dots = byTime.filter((deployment) => deployment.hashTypes!.includes(hashType));
            const rows = laneDotRows(dots.map((deployment) => x(deployment.timestampMs)));
            return (
              <g key={hashType} data-lane={hashType}>
                {narrow ? (
                  <>
                    <text
                      x="0"
                      y={y + 11}
                      fontSize="11"
                      fill="#dee2ec"
                      stroke="#0c0e15"
                      strokeWidth="3"
                      paintOrder="stroke"
                    >
                      {`${hashType} · ${LANE_TEXT[hashType]}`}
                    </text>
                    <text
                      x={width}
                      y={y + 11}
                      textAnchor="end"
                      fontSize="10.5"
                      fill="#a0a8be"
                      stroke="#0c0e15"
                      strokeWidth="3"
                      paintOrder="stroke"
                    >
                      {`${live} · ${lane.knownVersions.toLocaleString()} versions`}
                    </text>
                  </>
                ) : (
                  <>
                    <text x="0" y={y + 17} fontSize="12" fill="#dee2ec">
                      {`hash_type ${hashType}`}
                    </text>
                    <text x="0" y={y + 31} fontSize="10.5" fill="#7c849c">
                      {LANE_TEXT[hashType]}
                    </text>
                    <text x={width} y={y + 17} textAnchor="end" fontSize="12" fill="#dee2ec">
                      {live}
                    </text>
                    <text x={width} y={y + 31} textAnchor="end" fontSize="10.5" fill="#7c849c">
                      {versions}
                    </text>
                  </>
                )}
                {pinnedFrom !== null && (
                  <>
                    <line
                      x1={x(genesisMs)}
                      x2={x(pinnedFrom)}
                      y1={y + bar.y + bar.h / 2}
                      y2={y + bar.y + bar.h / 2}
                      stroke="#343c50"
                      strokeDasharray="2 4"
                    />
                    {!narrow && (
                      <text
                        x={(x(noteFrom) + x(pinnedFrom)) / 2}
                        y={y + 30}
                        textAnchor="middle"
                        fontSize="10"
                        fill="#4b5266"
                      >
                        not valid before {pinnedBy}
                      </text>
                    )}
                  </>
                )}
                {segments[hashType].map(([from, to, vm]) => (
                  <g key={vm}>
                    <rect
                      x={x(from) + 1}
                      y={y + bar.y}
                      width={Math.max(0, x(to) - x(from) - 2)}
                      height={bar.h}
                      rx="3"
                      fill={DATA_COLOR}
                      fillOpacity={VM_OPACITY[vm]}
                    />
                    <text x={x(from) + 6} y={y + bar.y + bar.h - 4} fontSize="10.5" fill="#dee2ec">
                      {vm}
                    </text>
                  </g>
                ))}
                {lane.knownVersions === 0 && (
                  <text
                    x={narrow ? x(segments[hashType][0][0]) - 6 : x(segments[hashType][0][0]) + 40}
                    y={y + bar.y + bar.h - 4}
                    textAnchor={narrow ? 'end' : 'start'}
                    fontSize="10.5"
                    fill="#dee2ec"
                  >
                    no known script yet
                  </text>
                )}
                {dots.map((deployment, k) => {
                  const active = deployment.versionHash === selected?.versionHash;
                  return (
                    <g
                      key={deployment.versionHash}
                      role="button"
                      tabIndex={0}
                      data-lane-dot={hashType}
                      aria-label={`${markName(deployment)}, ${hashType}, deployed ${formatUtc8Date(deployment.timestampMs)}`}
                      aria-pressed={active}
                      className="cursor-pointer outline-none"
                      onMouseEnter={() => setSelectedHash(deployment.versionHash)}
                      onFocus={() => setSelectedHash(deployment.versionHash)}
                      onClick={() => setSelectedHash(deployment.versionHash)}
                    >
                      <title>
                        {`${deployment.versionName} · deployed ${formatUtc8Date(deployment.timestampMs)}`}
                      </title>
                      <circle
                        cx={x(deployment.timestampMs)}
                        cy={y + bar.dots[rows[k]]}
                        r="3"
                        fill={active ? DATA_COLOR : '#0c0e15'}
                        stroke={active ? DATA_COLOR : '#dee2ec'}
                        strokeWidth="1"
                      />
                    </g>
                  );
                })}
              </g>
            );
          })}
          <text x={width} y={bottom + 32} textAnchor="end" fontSize="11" fill="#dee2ec">
            now · epoch {formatExactDecimal(String(tipEpoch), 'tip epoch')}
          </text>
        </svg>
      )}
      {unregistered.length > 0 && (
        <div className="mt-2">
          <ScriptDeploymentTimeline
            deployments={unregistered}
            domain={[genesisMs, tipMs]}
            inset={{ left, right }}
            label={`no registered reference · ${unregistered.length.toLocaleString()}`}
            selectedHash={selectedHash}
            onSelect={setSelectedHash}
            hideEvidence
          />
        </div>
      )}
      <DeploymentEvidence selected={selected} />
    </div>
  );
}
