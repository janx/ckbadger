import { cn } from '@/lib/utils';

/** The share at which a share bar is full (board L5): the top holders stay distinguishable. */
export const SHARE_BAR_FULL_PERCENT = 10;

/**
 * A bar's width for `amount` of `total`, drawn to 10 % = full; a larger share is a full bar and
 * its exact number stands beside it. Exact to 0.01 % of the bar from integers.
 */
export function shareBarWidth(amount: bigint, total: bigint): string {
  if (amount < BigInt(0) || total < BigInt(0) || amount > total)
    throw new Error(`Share bar: ${amount} of ${total} is not a share`);
  if (total === BigInt(0)) return '0%';
  const hundredths = (amount * BigInt(100 * 100 * 100)) / (total * BigInt(SHARE_BAR_FULL_PERCENT));
  return hundredths >= BigInt(10000) ? '100%' : `${Number(hundredths) / 100}%`;
}

/** The thin share bar of the holder rows: a track that fills the free width. */
export function ShareBar({
  width,
  color,
  className,
}: {
  width: string;
  color?: string;
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      data-share-bar={width}
      className={cn('bg-base-elevated flex h-1 min-w-10 flex-1 rounded-sm', className)}
    >
      <span
        className={cn('block h-full rounded-sm', !color && 'bg-interactive')}
        style={{ width, ...(color ? { backgroundColor: color } : {}) }}
      />
    </span>
  );
}
