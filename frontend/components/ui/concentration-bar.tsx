import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';
import { partitionSpans } from '@/lib/partition-geometry';

export interface ConcentrationGroup {
  label: string;
  value: bigint;
}
export interface ConcentrationBarProps {
  label: string;
  assetClass: AssetClass;
  total: bigint;
  /** Ranked groups, most concentrated first (largest · next 9 · next 90 · the rest). */
  groups: readonly ConcentrationGroup[];
}

const OPACITY = [1, 0.7, 0.45, 0.22];

/** How concentrated a holding is: one class colour, fading by rank (DAO-12; Token, Collection). */
export function ConcentrationBar({ label, assetClass, total, groups }: ConcentrationBarProps) {
  if (groups.length > OPACITY.length)
    throw new Error(`${label}: at most ${OPACITY.length} ranked groups, got ${groups.length}`);
  const spans = partitionSpans(
    groups.map((g) => g.value),
    total,
    0,
    100,
    label
  );
  return (
    <div className="min-w-0 space-y-2">
      <div
        role="img"
        aria-label={`${label}: ${groups.map((g) => `${g.label} ${formatPercent(g.value, total)}%`).join(', ')}`}
        className="flex h-3 w-full gap-0.5 overflow-hidden rounded-md"
      >
        {groups.map((g, i) => (
          <span
            key={g.label}
            data-segment={g.label}
            style={{
              width: `${spans[i].width}%`,
              opacity: OPACITY[i],
              backgroundColor: CLASS_COLORS[assetClass],
            }}
          />
        ))}
      </div>
      <div className="text-text-dim flex flex-wrap gap-x-6 gap-y-1 text-[12px]">
        {groups.map((g) => (
          <span key={g.label}>
            <span>{g.label}</span>{' '}
            <span className="text-text-bright tabular-nums">{formatPercent(g.value, total)}%</span>
          </span>
        ))}
      </div>
    </div>
  );
}
