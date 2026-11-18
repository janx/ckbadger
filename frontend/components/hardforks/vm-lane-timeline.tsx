'use client';

import { useElementWidth } from '@/hooks/useElementWidth';
import {
  DOT_HIT_PX,
  DeploymentEvidence,
  ScriptDeploymentTimeline,
  markName,
  useDeploymentReadout,
} from '@/components/hardforks/script-deployment-timeline';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatExactDecimal } from '@/lib/exact-integer';
import { formatCount, countNoun } from '@/lib/format-count';
import { formatUtc8Date } from '@/lib/utils';
import type { HardforkHashTypeLaneF2, HardforkScriptDeployment } from '@/lib/api';
import { SVG_TEXT, lineAscentPx, wrapWords } from '@/lib/chart-type';
import { advancePx, descentPx, tightLinePx } from '@/lib/type-metrics';

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
/** On a phone, the clear space kept above the first lane's header line for the rules. */
const LANE_HEAD_CLEAR = 4;
/** A glyph of an edition block's sub-line (the note size), for keeping labels apart. */
export const CHAR_PX = advancePx(SVG_TEXT.note);
/**
 * The plot's name, inside it at the top-left (hierarchy v3, R13): the page's former section title
 * and its hint line. It wraps after the title where one line does not fit (a phone).
 */
const PLOT_TITLE = 'which VM runs a script, over time';
const PLOT_HINT = 'dots: when each known script version was deployed';
/*
 * The figure's words stand in tight lines of their roles (lib/chart-type.ts), so its bands and
 * baselines follow from the roles (readability plan 2026-11-18, lane F3).
 */
/** The plot's name at the label size. */
const NAME_PX = SVG_TEXT.label;
export const NAME_CHAR_PX = advancePx(NAME_PX);
/** The hint under the title, a tight label line below it (15 px). */
export const PLOT_NAME_LINE = tightLinePx(NAME_PX);
/** The name's first line stands 2 px under the svg's top. */
const PLOT_NAME_PAD = 2;
const PLOT_NAME_Y = PLOT_NAME_PAD + lineAscentPx(NAME_PX);
/** Clear space between the name band and the edition labels under it. */
const PLOT_NAME_GAP = 5;
/** Glyph width of the edition names, set at the title size. */
export const EDITION_CHAR_PX = advancePx(SVG_TEXT.title);
/**
 * An edition block: its name (the title size) over its epoch (the note size), 6 px under the
 * name band: their baselines 18.45 and 33.9 under it (were 18 and 33, where the two lines met);
 * the clear space kept between two blocks side by side.
 */
const BLOCK_PAD = 6;
const EDITION_LINES = tightLinePx(SVG_TEXT.title) + tightLinePx(SVG_TEXT.note);
const NAME_Y = BLOCK_PAD + lineAscentPx(SVG_TEXT.title);
const SUB_Y = BLOCK_PAD + tightLinePx(SVG_TEXT.title) + lineAscentPx(SVG_TEXT.note);
const BLOCK_GAP = 8;
/**
 * A block that would crowd the one before it drops a whole row — name over epoch, together —
 * so its epoch never reads as the other edition's (390 px: Meepo's epoch sat under Mirana's).
 * A row is the block's two lines and 3 px: 34.5 px (was 34).
 */
const BLOCK_ROW = EDITION_LINES + 3;
/**
 * Under the edition rows, on the desktop, the count column's head ("live references", a note
 * line) 3 px under them and 8.5 px over the lanes; on a phone 24.5 px of air.
 */
const LIVE_HEAD = { gap: 3, over: 8.5 } as const;
const PHONE_AIR = 24.5;
/**
 * A desktop lane's head (the head size) over its note, 4 px under the lane's top: baselines 16.1
 * and 31.4 (were 17 and 31, where the two lines met).
 */
const LANE_PAD = 4;
const HEAD_Y = LANE_PAD + lineAscentPx(SVG_TEXT.head);
const NOTE_Y = LANE_PAD + tightLinePx(SVG_TEXT.head) + lineAscentPx(SVG_TEXT.note);
/** A phone lane's header line, at the lane's top. */
const HEADER_Y = lineAscentPx(SVG_TEXT.label);
/** The years under the lanes, 5 px under them, and "now" a label line under the years. */
const YEAR_GAP = 5;

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
  // The name takes one line where it fits; else the title over the hint, each broken between words
  // where it does not fit the plot either (the hint at 12 px is 352.8 px: a 360 phone's 328 px
  // plot takes it on two lines).
  const nameOneLine = `${PLOT_TITLE} · ${PLOT_HINT}`.length * NAME_CHAR_PX <= w;
  const nameLines = nameOneLine
    ? [`${PLOT_TITLE} · ${PLOT_HINT}`]
    : [...wrapWords(PLOT_TITLE, NAME_PX, w), ...wrapWords(PLOT_HINT, NAME_PX, w)];
  const band = PLOT_NAME_PAD + nameLines.length * PLOT_NAME_LINE + PLOT_NAME_GAP;
  const top =
    band +
    BLOCK_PAD +
    EDITION_LINES +
    (labelRows - 1) * BLOCK_ROW +
    (narrow ? PHONE_AIR : LIVE_HEAD.gap + tightLinePx(SVG_TEXT.note) + LIVE_HEAD.over);
  const laneH = narrow ? 64 : 46;
  /**
   * Where a lane's bar and its dot rows sit, from the lane's top: a bar holds its VM's note line
   * and half a pixel over and under it (16 px; a phone's was 14, under the 15 px line), on a
   * phone 3 px under the header line.
   */
  const barH = tightLinePx(SVG_TEXT.note) + 1;
  const bar = narrow
    ? { y: tightLinePx(SVG_TEXT.label) + 3, h: barH, dots: [42, 50] }
    : { y: 6, h: barH, dots: [30, 38] };
  /** A word in a bar stands on the line centred in it. */
  const barLabelY = (barH - tightLinePx(SVG_TEXT.note)) / 2 + lineAscentPx(SVG_TEXT.note);
  const bottom = top + laneH * LANE_ORDER.length;
  const yearY = bottom + YEAR_GAP + lineAscentPx(SVG_TEXT.label);
  const nowY = yearY + tightLinePx(SVG_TEXT.label);
  // the years' line and "now"'s, 5 px under the lanes and 9 px over the foot: 44 px as before
  const height = bottom + YEAR_GAP + 2 * tightLinePx(SVG_TEXT.label) + 9;
  /**
   * PB-02: the vertical spans a rule may run through. On a phone each lane's header line (its label
   * and count) is its own line, so the edition and year rules run behind the bars and dots only.
   */
  const laneSpans: [number, number][] = LANE_ORDER.map((_, i) => [
    top + i * laneH + bar.y - 2,
    top + (i + 1) * laneH - 6,
  ]);
  const ruleSpans = (from: number): [number, number][] =>
    narrow
      ? [
          ...(from < top - LANE_HEAD_CLEAR
            ? [[from, top - LANE_HEAD_CLEAR] as [number, number]]
            : []),
          ...laneSpans,
        ]
      : [[from, bottom]];
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
                {ruleSpans(top - 8).map(([y1, y2]) => (
                  <line key={y1} x1={at} x2={at} y1={y1} y2={y2} stroke="var(--c-tile)" />
                ))}
                <text
                  data-year
                  x={at + 3}
                  y={yearY}
                  fontSize={SVG_TEXT.label}
                  fill="var(--c-text-dim)"
                >
                  {year}
                </text>
              </g>
            );
          })}
          <text
            data-role="chart-name"
            x="0"
            y={PLOT_NAME_Y}
            fontSize={SVG_TEXT.label}
            fill="var(--c-text-dim)"
          >
            {nameOneLine
              ? nameLines[0]
              : nameLines.map((line, i) => (
                  <tspan key={line} x={i ? '0' : undefined} dy={i ? PLOT_NAME_LINE : undefined}>
                    {line}
                  </tspan>
                ))}
          </text>
          <text x={left} y={band + NAME_Y} fontSize={SVG_TEXT.title} fill="var(--c-text-bright)">
            genesis
          </text>
          <text x={left} y={band + SUB_Y} fontSize={SVG_TEXT.note} fill="var(--c-text-dim)">
            {genesisDate}
          </text>
          {editionLabels.map(({ edition, at, sub, turn, row }) => {
            return (
              <g key={edition.shortName}>
                {ruleSpans(band + 10).map(([y1, y2]) => (
                  <line
                    key={y1}
                    data-edition={edition.shortName}
                    x1={at}
                    x2={at}
                    y1={y1}
                    y2={y2}
                    stroke="var(--c-text-bright)"
                    strokeWidth="1.2"
                  />
                ))}
                <text
                  x={turn ? at - 6 : at + 6}
                  y={band + NAME_Y + row * BLOCK_ROW}
                  textAnchor={turn ? 'end' : 'start'}
                  fontSize={SVG_TEXT.title}
                  fill="var(--c-text-bright)"
                >
                  {edition.shortName}
                </text>
                <text
                  x={turn ? at - 6 : at + 6}
                  y={band + SUB_Y + row * BLOCK_ROW}
                  textAnchor={turn ? 'end' : 'start'}
                  fontSize={SVG_TEXT.note}
                  fill="var(--c-text-dim)"
                >
                  {sub}
                </text>
              </g>
            );
          })}
          {/* On a phone each lane's header carries its own count. */}
          {!narrow && (
            <text
              x={width}
              y={top - LIVE_HEAD.over - descentPx(SVG_TEXT.note)}
              textAnchor="end"
              fontSize={SVG_TEXT.note}
              fill="var(--c-text-dim)"
            >
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
            const versions = `${lane.knownVersions.toLocaleString()} known ${countNoun(lane.knownVersions, 'version')}`;
            const dots = byTime.filter((deployment) => deployment.hashTypes!.includes(hashType));
            const rows = laneDotRows(dots.map((deployment) => x(deployment.timestampMs)));
            return (
              <g key={hashType} data-lane={hashType}>
                {narrow ? (
                  <>
                    <text
                      x="0"
                      y={y + HEADER_Y}
                      fontSize={SVG_TEXT.label}
                      fill="var(--c-text-bright)"
                      stroke="var(--c-base-bg)"
                      strokeWidth="3"
                      paintOrder="stroke"
                    >
                      {`${hashType} · ${LANE_TEXT[hashType]}`}
                    </text>
                    <text
                      x={width}
                      y={y + HEADER_Y}
                      textAnchor="end"
                      fontSize={SVG_TEXT.note}
                      fill="var(--c-text)"
                      stroke="var(--c-base-bg)"
                      strokeWidth="3"
                      paintOrder="stroke"
                    >
                      {`${live} · ${
                        lane.knownVersions === 0
                          ? 'no known script yet'
                          : formatCount(lane.knownVersions, 'version')
                      }`}
                    </text>
                  </>
                ) : (
                  <>
                    <text x="0" y={y + HEAD_Y} fontSize={SVG_TEXT.head} fill="var(--c-text-bright)">
                      {`hash_type ${hashType}`}
                    </text>
                    <text x="0" y={y + NOTE_Y} fontSize={SVG_TEXT.note} fill="var(--c-text-dim)">
                      {LANE_TEXT[hashType]}
                    </text>
                    <text
                      x={width}
                      y={y + HEAD_Y}
                      textAnchor="end"
                      fontSize={SVG_TEXT.head}
                      fill="var(--c-text-bright)"
                    >
                      {live}
                    </text>
                    <text
                      x={width}
                      y={y + NOTE_Y}
                      textAnchor="end"
                      fontSize={SVG_TEXT.note}
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
                        y={y + bar.y + bar.h / 2 + 4 + lineAscentPx(SVG_TEXT.tick)}
                        textAnchor="middle"
                        fontSize={SVG_TEXT.tick}
                        fill="var(--c-text-dim)"
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
                      fill={DATA_COLOR}
                      fillOpacity={VM_OPACITY[vm]}
                    />
                    <text
                      x={x(from) + 6}
                      y={y + bar.y + barLabelY}
                      fontSize={SVG_TEXT.note}
                      fill="var(--c-text-bright)"
                    >
                      {vm}
                    </text>
                  </g>
                ))}
                {/* On a phone the empty lane says so on its header line (PB-02). */}
                {lane.knownVersions === 0 && !narrow && (
                  <text
                    x={x(segments[hashType][0][0]) + 40}
                    y={y + bar.y + barLabelY}
                    fontSize={SVG_TEXT.note}
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
                      aria-label={`${markName(deployment, 'row')}, ${hashType}, deployed ${formatUtc8Date(deployment.timestampMs)}`}
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
                {dots.length > 0 && (
                  <DotHitArea
                    lane={hashType}
                    x={left}
                    y={y + (bar.dots[0] + bar.dots[1]) / 2 - DOT_HIT_PX / 2}
                    width={w - left - right}
                    dots={dots.map((deployment, k) => ({
                      key: deployment.versionHash,
                      cx: x(deployment.timestampMs),
                      cy: y + bar.dots[rows[k]],
                    }))}
                    readout={readout}
                  />
                )}
              </g>
            );
          })}
          <text
            x={width}
            y={nowY}
            textAnchor="end"
            fontSize={SVG_TEXT.label}
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

/**
 * PB-02: a lane's version dots are read from one transparent band, DOT_HIT_PX tall, over their two
 * rows: the dot nearest the pointer answers a hover or a tap, so overlapping 6 px dots are each
 * reachable. The dots keep their own focus, Enter and Space for the keyboard.
 */
function DotHitArea({
  lane,
  x,
  y,
  width,
  dots,
  readout,
}: {
  lane: string;
  x: number;
  y: number;
  width: number;
  dots: readonly { key: string; cx: number; cy: number }[];
  readout: ReturnType<typeof useDeploymentReadout>;
}) {
  const nearest = (event: { clientX: number; clientY: number; currentTarget: Element }) => {
    const svg = event.currentTarget.closest('svg')!.getBoundingClientRect();
    const px = event.clientX - svg.left;
    const py = event.clientY - svg.top;
    return dots.reduce((best, dot) =>
      Math.hypot(dot.cx - px, dot.cy - py) < Math.hypot(best.cx - px, best.cy - py) ? dot : best
    ).key;
  };
  return (
    <rect
      data-dot-hit={lane}
      x={x}
      y={y}
      width={Math.max(0, width)}
      height={DOT_HIT_PX}
      fill="transparent"
      className="cursor-pointer"
      onPointerMove={(event) => readout.hover(nearest(event))}
      onPointerLeave={() => {
        // the readout is sticky: a hover outlasts the pointer unless a pin is waiting
        if (readout.pinned !== null) readout.hover(null);
      }}
      onClick={(event) => readout.toggle(nearest(event))}
    />
  );
}
