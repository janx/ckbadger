import { niceTicks } from '@/lib/chart-ticks';

interface BlockContentsProps {
  transactionSizes: readonly number[];
  proposalsCount: number;
  unclesCount: number;
}

/** The same square/diamond/dot grammar as the transaction pipeline. */
export function BlockContents({
  transactionSizes,
  proposalsCount,
  unclesCount,
}: BlockContentsProps) {
  if (
    !transactionSizes.length ||
    transactionSizes.some((size) => !Number.isSafeInteger(size) || size <= 0)
  ) {
    throw new Error('Block contents requires the exact positive size of every transaction');
  }
  const userSizes = transactionSizes.slice(1);
  const largest = Math.max(...transactionSizes);
  const shown = userSizes.slice(0, 7);
  const label = `Cellbase, ${userSizes.length} user transactions, ${proposalsCount} proposals, ${unclesCount} uncles`;
  return (
    <span className="inline-flex flex-wrap items-center gap-2" title={label}>
      <svg
        role="img"
        aria-label={label}
        viewBox="0 0 112 18"
        width="112"
        height="18"
        className="shrink-0"
      >
        <path d="M5 4.5 9.5 9 5 13.5.5 9z" fill="#2edba3" />
        {shown.map((size, index) => {
          const side = Math.sqrt(size / largest) * 10;
          return (
            <rect
              key={index}
              data-transaction-size={size}
              x={14 + index * 13}
              y={(18 - side) / 2}
              width={side}
              height={side}
              fill="#2edba3"
              opacity="0.8"
            />
          );
        })}
        {proposalsCount > 0 && <circle cx="108" cy="9" r="2.4" fill="#f2c55c" />}
      </svg>
      <span className="text-text-dim text-[11px]">
        {transactionSizes.length.toLocaleString('en-US')} tx
        {userSizes.length > shown.length ? ` · +${userSizes.length - shown.length} tiles` : ''}
      </span>
    </span>
  );
}

interface ResourceBarProps {
  label: string;
  value: number;
  scale: number;
  overhead?: number;
  /** Keep the exact count above its bar when a table column is narrow. */
  stacked?: boolean;
}

/** Geometry uses numbers; the printed resource count stays an exact integer. */
export function ResourceBar({ label, value, scale, overhead, stacked = false }: ResourceBarProps) {
  if (
    !Number.isSafeInteger(value) ||
    value < 0 ||
    !Number.isFinite(scale) ||
    scale <= 0 ||
    value > scale
  ) {
    throw new Error(`${label}: invalid resource value ${value} on scale ${scale}`);
  }
  if (
    overhead !== undefined &&
    (!Number.isSafeInteger(overhead) || overhead < 0 || overhead > value)
  ) {
    throw new Error(`${label}: overhead ${overhead} exceeds resource value ${value}`);
  }
  return (
    <span
      className={
        stacked
          ? 'flex min-w-0 flex-col items-end gap-1.5'
          : 'flex min-w-0 items-center justify-end gap-3'
      }
    >
      <span
        role="img"
        aria-label={`${label}: ${value.toLocaleString('en-US')}${overhead === undefined ? '' : `, ${overhead.toLocaleString('en-US')} overhead, ${(value - overhead).toLocaleString('en-US')} user transactions`}`}
        className="bg-base-elevated flex h-2 w-20 max-w-full shrink-0 overflow-hidden"
      >
        {overhead !== undefined && (
          <span className="bg-[#3a4258]" style={{ width: `${(overhead / scale) * 100}%` }} />
        )}
        <span
          className="bg-jade"
          style={{
            width: `${((overhead === undefined ? value : value - overhead) / scale) * 100}%`,
          }}
        />
      </span>
      <span
        className={
          stacked ? 'order-first whitespace-nowrap tabular-nums' : 'whitespace-nowrap tabular-nums'
        }
      >
        {value.toLocaleString('en-US')}
      </span>
    </span>
  );
}

export function resourceScale(values: readonly number[]): number {
  if (values.some((n) => !Number.isSafeInteger(n) || n < 0))
    throw new Error('Resource scale requires nonnegative exact integers');
  const max = values.length ? Math.max(...values) : 0;
  const ticks = niceTicks(0, max);
  return ticks.values[ticks.values.length - 1];
}
