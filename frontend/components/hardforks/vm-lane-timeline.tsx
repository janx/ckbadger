'use client';

import { useElementWidth } from '@/hooks/useElementWidth';
import { ScriptDeploymentTimeline } from '@/components/hardforks/script-deployment-timeline';
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
const LANE_TEXT: Record<(typeof LANE_ORDER)[number], string> = {
  type: 'the newest VM',
  data1: 'pinned to VM1',
  data2: 'pinned to VM2',
  data: 'pinned to VM0',
};
/** The VMs are one hue, deeper for newer: the board's encoding. */
const VM_OPACITY = { VM0: 0.16, VM1: 0.32, VM2: 0.55 } as const;
const UTC8_MS = 8 * 3_600_000;
const TOP = 64;
const LANE_H = 46;

/** An edition's activation day as an instant on the axis: that UTC+8 day's start. */
function dayStartMs(day: string): number {
  const ms = Date.parse(`${day}T00:00:00+08:00`);
  if (!Number.isFinite(ms)) throw new Error(`Invalid hardfork activation date ${day}`);
  return ms;
}

/**
 * Which VM runs a script, over time (CH-40): genesis to now with the two edition lines, one lane
 * per hash type with its VM segments, and each lane's live references from R11 on the right
 * (CH-41). The registry's deployments sit under the lanes on the same axis. Lanes follow the
 * protocol rule — `type` follows the newest VM, each data hash type pins one — and nothing more.
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
  if (!(genesisMs < tipMs)) throw new Error('VM lane timeline needs a tip after genesis');
  if (lanes.map((lane) => lane.hashType).join() !== LANE_ORDER.join())
    throw new Error(`hash-type lanes out of VM order: ${lanes.map((l) => l.hashType).join()}`);
  const [vm1, vm2] = editions;
  const t1 = dayStartMs(vm1.activationDate);
  const t2 = dayStartMs(vm2.activationDate);
  if (!(genesisMs < t1 && t1 < t2 && t2 <= tipMs))
    throw new Error('hardfork activations must lie in order between genesis and the tip');
  const narrow = width !== null && width < 640;
  const left = narrow ? 96 : 190;
  const right = narrow ? 88 : 150;
  const height = TOP + LANE_H * LANE_ORDER.length + 44;
  const bottom = TOP + LANE_H * LANE_ORDER.length;
  const x = (t: number) =>
    left + ((t - genesisMs) / (tipMs - genesisMs)) * ((width ?? 0) - left - right);
  const firstYear = new Date(genesisMs + UTC8_MS).getUTCFullYear() + 1;
  const lastYear = new Date(tipMs + UTC8_MS).getUTCFullYear();
  const years = Array.from(
    { length: Math.max(0, lastYear - firstYear + 1) },
    (_, i) => firstYear + i
  );
  const segments: Record<(typeof LANE_ORDER)[number], [number, number, keyof typeof VM_OPACITY][]> =
    {
      type: [
        [genesisMs, t1, 'VM0'],
        [t1, t2, 'VM1'],
        [t2, tipMs, 'VM2'],
      ],
      data1: [[t1, tipMs, 'VM1']],
      data2: [[t2, tipMs, 'VM2']],
      data: [[genesisMs, tipMs, 'VM0']],
    };
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
            const at = x(Date.UTC(year, 0, 1) - UTC8_MS);
            return (
              <g key={year}>
                <line x1={at} x2={at} y1={TOP - 8} y2={bottom} stroke="#141826" />
                <text x={at + 3} y={bottom + 16} fontSize="11" fill="#7c849c">
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
          {editions.map((edition) => {
            const at = x(dayStartMs(edition.activationDate));
            const epoch = `epoch ${formatExactDecimal(String(edition.activationEpoch), 'edition epoch')}`;
            return (
              <g key={edition.shortName}>
                <line x1={at} x2={at} y1="10" y2={bottom} stroke="#dee2ec" strokeWidth="1.2" />
                <text x={at + 6} y="18" fontSize="12.5" fill="#dee2ec">
                  {edition.shortName}
                </text>
                <text x={at + 6} y="33" fontSize="10.5" fill="#7c849c">
                  {narrow ? epoch : `${epoch} · ${edition.activationDate}`}
                </text>
              </g>
            );
          })}
          <text x={width} y={TOP - 12} textAnchor="end" fontSize="10.5" fill="#7c849c">
            live references
          </text>
          {LANE_ORDER.map((hashType, i) => {
            const y = TOP + i * LANE_H;
            const lane = lanes[i];
            const pinnedFrom = hashType === 'data1' ? t1 : hashType === 'data2' ? t2 : null;
            const pinnedBy = hashType === 'data1' ? vm1.shortName : vm2.shortName;
            return (
              <g key={hashType} data-lane={hashType}>
                <text x="0" y={y + 17} fontSize={narrow ? 11 : 12} fill="#dee2ec">
                  {narrow ? hashType : `hash_type ${hashType}`}
                </text>
                <text x="0" y={y + 31} fontSize="10.5" fill="#7c849c">
                  {LANE_TEXT[hashType]}
                </text>
                {pinnedFrom !== null && (
                  <>
                    <line
                      x1={x(genesisMs)}
                      x2={x(pinnedFrom)}
                      y1={y + 14}
                      y2={y + 14}
                      stroke="#343c50"
                      strokeDasharray="2 4"
                    />
                    {!narrow && (
                      <text
                        x={(x(genesisMs) + x(pinnedFrom)) / 2}
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
                      y={y + 6}
                      width={Math.max(0, x(to) - x(from) - 2)}
                      height="16"
                      rx="3"
                      fill={DATA_COLOR}
                      fillOpacity={VM_OPACITY[vm]}
                    />
                    <text x={x(from) + 8} y={y + 18} fontSize="10.5" fill="#dee2ec">
                      {vm}
                    </text>
                  </g>
                ))}
                {lane.knownVersions === 0 && (
                  <text
                    x={narrow ? x(segments[hashType][0][0]) - 6 : x(segments[hashType][0][0]) + 40}
                    y={y + 18}
                    textAnchor={narrow ? 'end' : 'start'}
                    fontSize="10.5"
                    fill="#dee2ec"
                  >
                    no known script yet
                  </text>
                )}
                <text x={width} y={y + 17} textAnchor="end" fontSize="12" fill="#dee2ec">
                  {formatExactDecimal(lane.liveReferences, `${hashType} live references`)}
                </text>
                <text x={width} y={y + 31} textAnchor="end" fontSize="10.5" fill="#7c849c">
                  {lane.knownVersions.toLocaleString()} known version
                  {lane.knownVersions === 1 ? '' : 's'}
                </text>
              </g>
            );
          })}
          <text x={width} y={bottom + 32} textAnchor="end" fontSize="11" fill="#dee2ec">
            now · epoch {formatExactDecimal(String(tipEpoch), 'tip epoch')}
          </text>
        </svg>
      )}
      <div className="mt-2">
        <ScriptDeploymentTimeline
          deployments={deployments}
          domain={[genesisMs, tipMs]}
          inset={{ left, right }}
        />
      </div>
    </div>
  );
}
