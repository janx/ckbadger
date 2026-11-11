'use client';

import { useElementWidth } from '@/hooks/useElementWidth';
import {
  DeploymentEvidence,
  ScriptDeploymentTimeline,
  markName,
  useDeploymentReadout,
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
/**
 * The plot's name, inside it at the top-left (hierarchy v3, R13): the page's former section title
 * and its hint line. It wraps after the title where one line does not fit (a phone).
 */
const PLOT_TITLE = 'which VM runs a script, over time';
const PLOT_HINT = 'dots: when each known script version was deployed';
/** 11 px mono advances ~0.64 em in Chromium (lane D, observed). */
const NAME_PX = 11;
const NAME_CHAR_PX = NAME_PX * 0.64;
const PLOT_NAME_Y = 13;
const PLOT_NAME_LINE = 14;
/** Clear space between the name band and the edition labels under it. */
const PLOT_NAME_GAP = 9;
/** Glyph width of the 12.5 px edition names. */
const EDITION_CHAR_PX = 7.5;
/** Baselines of an edition block's name and epoch on the first row, and the clear space kept
 * between two blocks side by side. */
const NAME_Y = 18;
const SUB_Y = 33;
const BLOCK_GAP = 8;
/**
 * A block that would crowd the one before it drops a whole row — name over epoch, together —
 * so its epoch never reads as the other edition's (390 px: Meepo's epoch sat under Mirana's).
 * A row's name baseline is 34 px under the row above it: 19 px clear of that row's epoch.
 */
const BLOCK_ROW = 34;

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
  // One readout for lane dots and strip dots (R6): hover names a version, a tap pins it.
  const readout = useDeploymentReadout();
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
  const w = width ?? 0;
  const x = (t: number) => left + ((t - genesisMs) / (tipMs - genesisMs)) * (w - left - right);
  // Edition label blocks (name over epoch): a block that would run off the right edge turns left
  // of its rule, and a block that would touch one already placed takes the next row down.
  const genesisDate = formatUtc8Date(genesisMs);
  const genesisBlock = {
    row: 0,
    x0: left,
    x1: left + Math.max('genesis'.length * EDITION_CHAR_PX, genesisDate.length * CHAR_PX),
  };
  const editionLabels = editions.reduce<
    {
      edition: EditionMark;
      at: number;
      sub: string;
      turn: boolean;
      row: number;
      x0: number;
      x1: number;
    }[]
  >((placed, edition) => {
    const at = x(dayStartMs(edition.activationDate));
    const epoch = `epoch ${formatExactDecimal(String(edition.activationEpoch), 'edition epoch')}`;
    const sub = narrow ? epoch : `${epoch} · ${edition.activationDate}`;
    const span = Math.max(edition.shortName.length * EDITION_CHAR_PX, sub.length * CHAR_PX);
    const turn = at + 6 + span > w;
    const x0 = turn ? at - 6 - span : at + 6;
    const taken = [genesisBlock, ...placed];
    let row = 0;
    while (
      taken.some((b) => b.row === row && x0 < b.x1 + BLOCK_GAP && b.x0 < x0 + span + BLOCK_GAP)
    )
      row++;
    return [...placed, { edition, at, sub, turn, row, x0, x1: x0 + span }];
  }, []);
  const labelRows = 1 + Math.max(0, ...editionLabels.map((l) => l.row));
  // The name takes one line where it fits, else the title over the hint; the labels start under it.
  const nameOneLine = `${PLOT_TITLE} · ${PLOT_HINT}`.length * NAME_CHAR_PX <= w;
  const band = PLOT_NAME_Y + (nameOneLine ? 0 : PLOT_NAME_LINE) + PLOT_NAME_GAP;
  const top = band + (narrow ? 62 : 64) + (labelRows - 1) * BLOCK_ROW;
  const laneH = narrow ? 64 : 46;
  /** Where a lane's bar and its dot rows sit, from the lane's top. */
  const bar = narrow ? { y: 18, h: 14, dots: [40, 48] } : { y: 6, h: 16, dots: [30, 38] };
  const bottom = top + laneH * LANE_ORDER.length;
  const height = bottom + 44;
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
  const byTime = [...deployments].sort(
    (a, b) => a.timestampMs - b.timestampMs || a.versionHash.localeCompare(b.versionHash)
  );
  const unregistered = byTime.filter((deployment) => deployment.hashTypes!.length === 0);
  const selected = deployments.find((d) => d.versionHash === readout.active) ?? null;
  // A reading names a version on the timeline: one the deployments no longer hold lets go.
  if (readout.active !== null && selected === null) readout.clear();
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
                <line x1={at} x2={at} y1={top - 8} y2={bottom} stroke="var(--c-tile)" />
                <text data-year x={at + 3} y={bottom + 16} fontSize="11" fill="var(--c-text-dim)">
                  {year}
                </text>
              </g>
            );
          })}
          <text
            data-role="chart-name"
            x="0"
            y={PLOT_NAME_Y}
            fontSize={NAME_PX}
            fill="var(--c-text-dim)"
          >
            {nameOneLine ? (
              `${PLOT_TITLE} · ${PLOT_HINT}`
            ) : (
              <>
                <tspan>{PLOT_TITLE}</tspan>
                <tspan x="0" dy={PLOT_NAME_LINE}>
                  {PLOT_HINT}
                </tspan>
              </>
            )}
          </text>
          <text x={left} y={band + NAME_Y} fontSize="12.5" fill="var(--c-text-bright)">
            genesis
          </text>
          <text x={left} y={band + SUB_Y} fontSize="10.5" fill="var(--c-text-dim)">
            {genesisDate}
          </text>
          {editionLabels.map(({ edition, at, sub, turn, row }) => {
            return (
              <g key={edition.shortName}>
                <line
                  data-edition={edition.shortName}
                  x1={at}
                  x2={at}
                  y1={band + 10}
                  y2={bottom}
                  stroke="var(--c-text-bright)"
                  strokeWidth="1.2"
                />
                <text
                  x={turn ? at - 6 : at + 6}
                  y={band + NAME_Y + row * BLOCK_ROW}
                  textAnchor={turn ? 'end' : 'start'}
                  fontSize="12.5"
                  fill="var(--c-text-bright)"
                >
                  {edition.shortName}
                </text>
                <text
                  x={turn ? at - 6 : at + 6}
                  y={band + SUB_Y + row * BLOCK_ROW}
                  textAnchor={turn ? 'end' : 'start'}
                  fontSize="10.5"
                  fill="var(--c-text-dim)"
                >
                  {sub}
                </text>
              </g>
            );
          })}
          {/* On a phone each lane's header carries its own count. */}
          {!narrow && (
            <text x={width} y={top - 12} textAnchor="end" fontSize="10.5" fill="var(--c-text-dim)">
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
                      fill="var(--c-text-bright)"
                      stroke="var(--c-base-bg)"
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
                      fill="var(--c-text)"
                      stroke="var(--c-base-bg)"
                      strokeWidth="3"
                      paintOrder="stroke"
                    >
                      {`${live} · ${lane.knownVersions.toLocaleString()} versions`}
                    </text>
                  </>
                ) : (
                  <>
                    <text x="0" y={y + 17} fontSize="12" fill="var(--c-text-bright)">
                      {`hash_type ${hashType}`}
                    </text>
                    <text x="0" y={y + 31} fontSize="10.5" fill="var(--c-text-dim)">
                      {LANE_TEXT[hashType]}
                    </text>
                    <text
                      x={width}
                      y={y + 17}
                      textAnchor="end"
                      fontSize="12"
                      fill="var(--c-text-bright)"
                    >
                      {live}
                    </text>
                    <text
                      x={width}
                      y={y + 31}
                      textAnchor="end"
                      fontSize="10.5"
                      fill="var(--c-text-dim)"
                    >
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
                      stroke="var(--c-text-ghost)"
                      strokeDasharray="2 4"
                    />
                    {!narrow && (
                      <text
                        x={(x(noteFrom) + x(pinnedFrom)) / 2}
                        y={y + 30}
                        textAnchor="middle"
                        fontSize="10"
                        fill="var(--c-ink-quiet)"
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
                    <text
                      x={x(from) + 6}
                      y={y + bar.y + bar.h - 4}
                      fontSize="10.5"
                      fill="var(--c-text-bright)"
                    >
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
                    fill="var(--c-text-bright)"
                  >
                    no known script yet
                  </text>
                )}
                {dots.map((deployment, k) => {
                  const active = deployment.versionHash === selected?.versionHash;
                  return (
                    <g
                      key={deployment.versionHash}
                      {...readout.bind(deployment.versionHash)}
                      data-lane-dot={hashType}
                      aria-label={`${markName(deployment)}, ${hashType}, deployed ${formatUtc8Date(deployment.timestampMs)}`}
                      className="cursor-pointer"
                    >
                      <title>
                        {`${deployment.versionName} · deployed ${formatUtc8Date(deployment.timestampMs)}`}
                      </title>
                      <circle
                        cx={x(deployment.timestampMs)}
                        cy={y + bar.dots[rows[k]]}
                        r="3"
                        fill={active ? DATA_COLOR : 'var(--c-base-bg)'}
                        stroke={active ? DATA_COLOR : 'var(--c-text-bright)'}
                        strokeWidth="1"
                      />
                    </g>
                  );
                })}
              </g>
            );
          })}
          <text
            x={width}
            y={bottom + 32}
            textAnchor="end"
            fontSize="11"
            fill="var(--c-text-bright)"
          >
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
            readout={readout}
            hideEvidence
          />
        </div>
      )}
      <DeploymentEvidence
        selected={selected}
        pinned={selected !== null && readout.isPinned(selected.versionHash)}
      />
    </div>
  );
}
