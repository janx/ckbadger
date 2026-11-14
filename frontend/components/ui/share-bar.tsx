import { cn } from '@/lib/utils';

/**
 * Ruling R7 (UI review 2026-11-14): a holder list draws its bars on one scale in both forms — the
 * desktop column's rounded maximum (`resourceScale` of the page's values, the smallest round value
 * at or above the largest), marked on its axis. A phone track has a fixed width, so every track of
 * a list ends at one x: 120 px in the two-line row, 64 px in the one-line row of the 640 band.
 */
export const PHONE_SHARE_TRACK = 120;
export const WIDE_SHARE_TRACK = 64;

/**
 * An API's exact percentage text ("1.25" = 1.25 %) as an integer of hundredths of a percent, the
 * unit a share bar is drawn in; anything but a plain decimal with at most two places is refused.
 */
export function percentHundredths(percent: string): number {
  const match = /^(\d+)(?:\.(\d{1,2}))?$/.exec(percent);
  if (!match) throw new Error(`Share bar: ${JSON.stringify(percent)} is not a percentage`);
  const hundredths = Number(match[1]) * 100 + Number((match[2] ?? '').padEnd(2, '0'));
  if (hundredths > 10000) throw new Error(`Share bar: ${percent} % is not a share`);
  return hundredths;
}

/** A share scale's axis mark: hundredths of a percent as a percentage ("60%", "0.5%"). */
export function formatShareTick(hundredths: number): string {
  if (!Number.isSafeInteger(hundredths) || hundredths < 0)
    throw new Error(`Share bar: invalid scale ${hundredths}`);
  return `${hundredths / 100}%`;
}

/** The fill of `value` on `scale`, in % of the track, to 0.01. */
export function shareBarWidth(value: number, scale: number): string {
  if (!Number.isSafeInteger(scale) || scale <= 0)
    throw new Error(`Share bar: invalid scale ${scale}`);
  if (!Number.isSafeInteger(value) || value < 0 || value > scale)
    throw new Error(`Share bar: ${value} is not on the scale ${scale}`);
  return `${Math.round((value / scale) * 10000) / 100}%`;
}

/**
 * The thin bar of a holder phone row: `value` on the list's `scale`, in the class colour, on the
 * fixed track. A nonzero value is at least 2 px, so it is still a mark (as the table's bars).
 */
export function ShareBar({
  value,
  scale,
  color,
  track,
  className,
}: {
  value: number;
  scale: number;
  /** The class colour of the list. */
  color: string;
  /** The track's fixed width in px. */
  track: number;
  className?: string;
}) {
  const width = shareBarWidth(value, scale);
  return (
    <span
      aria-hidden="true"
      data-share-track
      className={cn('bg-base-elevated flex h-1 flex-none rounded-sm', className)}
      style={{ width: `${track}px` }}
    >
      <span
        data-share-bar={width}
        className="block h-full rounded-sm"
        style={{ width, minWidth: value > 0 ? 2 : 0, backgroundColor: color }}
      />
    </span>
  );
}

/** The list's axis mark, as wide as its track: 0 at the track's start, the scale at its end. */
export function ShareAxis({ max, track }: { max: string; track: number }) {
  return (
    <span aria-hidden="true" data-share-axis className="flex flex-none">
      <span data-share-axis-track className="flex justify-between" style={{ width: `${track}px` }}>
        <span>0</span>
        <span>{max}</span>
      </span>
    </span>
  );
}
