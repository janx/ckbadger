'use client';

import { CLASS_COLORS, CLASS_KEYS, type AssetClass } from '@/lib/design-tokens';
import { color, fade, inkOnFill } from '@/lib/palette';
import { useTheme } from '@/hooks/useTheme';
import { partitionSpans } from '@/lib/partition-geometry';
import { useElementWidth } from '@/hooks/useElementWidth';
import { cn } from '@/lib/utils';
import { percentOf } from '@/lib/format-unit';

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
  // ---- hierarchy v3, DAO page (R15; additive, off = the thin bar with a legend row) ----
  /**
   * A 28 px bar with each group's name and share written inside its segment where they fit; the
   * legend row is drawn only when a segment cannot hold its words.
   */
  labelsInside?: boolean;
}

/** The ranked groups' fills: the class colour at these alphas over the page ground. */
export const CONCENTRATION_STEPS = [1, 0.7, 0.45, 0.22] as const;
/** The in-segment words: 12 px with 10 px padding each side. */
const LABEL_PX = 12;
const LABEL_PAD = 10;
/** A monospace advance: whole pixels where the browser snaps them (Chromium), else 0.6 em. */
const advance = (px: number) => Math.max(Math.round(0.6 * px), 0.6 * px);

/** The class colour at `alpha` as a background, so a segment's words keep their full ink. */
const withAlpha = fade;

/** How concentrated a holding is: one class colour, fading by rank (DAO-12; Token, Collection). */
export function ConcentrationBar({
  label,
  assetClass,
  total,
  groups,
  labelsInside = false,
}: ConcentrationBarProps) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const { resolved: theme } = useTheme();
  if (groups.length > CONCENTRATION_STEPS.length)
    throw new Error(
      `${label}: at most ${CONCENTRATION_STEPS.length} ranked groups, got ${groups.length}`
    );
  const spans = partitionSpans(
    groups.map((g) => g.value),
    total,
    0,
    100,
    label
  );
  const share = (g: ConcentrationGroup) => percentOf(g.value, total);
  const words = (g: ConcentrationGroup, i: number) =>
    i === 0 ? `${g.label} ${share(g)}` : `${g.label} · ${share(g)}`;
  const fits = (text: string, i: number) =>
    width !== null &&
    (width * spans[i].width) / 100 >= text.length * advance(LABEL_PX) + 2 * LABEL_PAD;
  const inside = labelsInside
    ? groups.map((g, i) => (fits(words(g, i), i) ? words(g, i) : ''))
    : [];
  const legend = !labelsInside || inside.some((w) => w === '');
  return (
    <div ref={ref} className="min-w-0 space-y-2">
      <div
        role="img"
        aria-label={`${label}: ${groups.map((g) => `${g.label} ${share(g)}`).join(', ')}`}
        className={cn(
          'flex w-full gap-0.5 overflow-hidden',
          labelsInside ? 'text-small h-[28px] rounded tabular-nums' : 'h-3'
        )}
      >
        {groups.map((g, i) =>
          labelsInside ? (
            <span
              key={g.label}
              data-segment={g.label}
              className="flex items-center overflow-hidden whitespace-nowrap rounded px-2.5"
              style={{
                width: `${spans[i].width}%`,
                backgroundColor: withAlpha(CLASS_COLORS[assetClass], CONCENTRATION_STEPS[i]),
                // DY-03: the words take the ink the fill's own luminance calls for, in this theme.
                color: color(inkOnFill(theme, CLASS_KEYS[assetClass], CONCENTRATION_STEPS[i]).ink),
              }}
            >
              {inside[i] && <span>{inside[i]}</span>}
            </span>
          ) : (
            <span
              key={g.label}
              data-segment={g.label}
              style={{
                width: `${spans[i].width}%`,
                opacity: CONCENTRATION_STEPS[i],
                backgroundColor: CLASS_COLORS[assetClass],
              }}
            />
          )
        )}
      </div>
      {legend && (
        <div
          data-role="concentration-legend"
          className="text-text-dim text-small flex flex-wrap gap-x-6 gap-y-1"
        >
          {groups.map((g) => (
            <span key={g.label}>
              <span>{g.label}</span>{' '}
              <span className="text-text-bright tabular-nums">{share(g)}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
