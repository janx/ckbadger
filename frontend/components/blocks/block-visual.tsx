import { AlignedBar } from '@/components/ui/aligned-bar';
import { niceTicks } from '@/lib/chart-ticks';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { formatPercent } from '@/lib/ckb-decimal';

const PROPOSAL_COLOR = CLASS_COLORS.dao;
const GLYPH_WIDTH = 112;
const DOT_STEP = 6;
const DOT_R = 2.4;

interface BlockContentsProps {
  transactionSizes: readonly number[];
  proposalsCount: number;
  unclesCount: number;
  /** False in a chain-strip tile: the glyph alone, counts stay in its accessible name. */
  showCount?: boolean;
}

/** The same square/diamond/dot grammar as the transaction pipeline. */
export function BlockContents({
  transactionSizes,
  proposalsCount,
  unclesCount,
  showCount = true,
}: BlockContentsProps) {
  if (
    !transactionSizes.length ||
    transactionSizes.some((size) => !Number.isSafeInteger(size) || size <= 0)
  ) {
    throw new Error('Block contents requires the exact positive size of every transaction');
  }
  if (!Number.isSafeInteger(proposalsCount) || proposalsCount < 0)
    throw new Error(`Block contents: invalid proposal count ${proposalsCount}`);
  const userSizes = transactionSizes.slice(1);
  const largest = Math.max(...transactionSizes);
  const shown = userSizes.slice(0, 7);
  // One gold dot per proposal after the squares (BA-03); the rest are counted in words.
  const dotsFrom = 14 + shown.length * 13;
  const dotRoom = Math.floor((GLYPH_WIDTH - dotsFrom - 2 * DOT_R) / DOT_STEP) + 1;
  const dots = Math.min(proposalsCount, Math.max(dotRoom, 0));
  const label = `Cellbase, ${userSizes.length} user transactions, ${proposalsCount} proposals, ${unclesCount} uncles`;
  return (
    <span className="inline-flex flex-wrap items-center gap-x-2 gap-y-0.5" title={label}>
      <span data-contents-line className="inline-flex items-center gap-2 whitespace-nowrap">
        <svg
          role="img"
          aria-label={label}
          viewBox={`0 0 ${GLYPH_WIDTH} 18`}
          width={GLYPH_WIDTH}
          height="18"
          className="shrink-0"
        >
          <path d="M5 4.5 9.5 9 5 13.5.5 9z" fill={DATA_COLOR} />
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
                fill={DATA_COLOR}
                opacity="0.8"
              />
            );
          })}
          {Array.from({ length: dots }, (_, i) => (
            <circle
              key={`p${i}`}
              data-proposal-dot
              cx={dotsFrom + DOT_R + i * DOT_STEP}
              cy="9"
              r={DOT_R}
              fill={PROPOSAL_COLOR}
            />
          ))}
        </svg>
        {showCount && (
          <span className="text-text-dim text-[11px]">
            {transactionSizes.length.toLocaleString('en-US')} tx
          </span>
        )}
      </span>
      {showCount && (userSizes.length > shown.length || proposalsCount > dots) && (
        <span className="text-text-dim text-[11px]">
          {[
            ...(userSizes.length > shown.length
              ? [`+${userSizes.length - shown.length} tiles`]
              : []),
            ...(proposalsCount > dots
              ? [`${proposalsCount.toLocaleString('en-US')} proposed`]
              : []),
          ].join(' · ')}
        </span>
      )}
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
  // Inline: the shared aligned bar, so every row starts at one left baseline (BA-01).
  if (!stacked)
    return (
      <AlignedBar
        label={label}
        scale={scale}
        segments={
          overhead === undefined
            ? [{ key: 'value', label, value, tone: 'data' }]
            : [
                { key: 'overhead', label: 'overhead', value: overhead, tone: 'overhead' },
                {
                  key: 'user',
                  label: 'user transactions',
                  value: value - overhead,
                  tone: 'data',
                },
              ]
        }
      />
    );
  return (
    <span className="flex min-w-0 flex-col items-end gap-1.5">
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

// ---- F2 blocks (BA-02, BA-04): the column scale and the consensus-limit footnote ----

/** "Size, bytes" over "0 … 4,000": the column's shared scale, as wide as its bar track. */
export function ScaleHeader({
  label,
  scale,
  trackWidth = 120,
}: {
  label: string;
  scale: number;
  trackWidth?: number;
}) {
  if (!Number.isSafeInteger(scale) || scale <= 0)
    throw new Error(`${label}: invalid column scale ${scale}`);
  return (
    <span className="flex flex-col gap-[3px]">
      <span>{label}</span>
      <span
        data-scale-ticks
        aria-hidden="true"
        className="flex justify-between text-[9.5px]"
        style={{ width: `${trackWidth}px` }}
      >
        <span>0</span>
        <span>{scale.toLocaleString('en-US')}</span>
      </span>
    </span>
  );
}

interface LimitRow {
  number: number;
  size: number;
  cycles: string | null;
  maxBlockBytes: number;
  maxBlockCycles: string;
}

/**
 * Blocks v3: the limits are a footnote, not a scale — the fullest block of the page and the
 * one with the most work, each as an exact share of the consensus limit the API reports.
 */
export function BlocksLimitsFootnote({ blocks }: { blocks: readonly LimitRow[] }) {
  if (!blocks.length) return null;
  const [{ maxBlockBytes, maxBlockCycles }] = blocks;
  for (const b of blocks)
    if (b.maxBlockBytes !== maxBlockBytes || b.maxBlockCycles !== maxBlockCycles)
      throw new Error(`Block ${b.number}: consensus limits differ from the rest of the page`);
  const fullest = blocks.reduce((a, b) => (b.size > a.size ? b : a));
  const run = blocks.filter((b) => b.cycles !== null);
  const most = run.length
    ? run.reduce((a, b) => (BigInt(b.cycles!) > BigInt(a.cycles!) ? b : a))
    : null;
  const bytesLimit = BigInt(maxBlockBytes);
  const cyclesLimit = BigInt(maxBlockCycles);
  return (
    <p className="text-text-dim text-xs">
      Size and cycles · Fullest of these {blocks.length.toLocaleString('en-US')}:{' '}
      <span className="text-text-bright">{fullest.size.toLocaleString('en-US')}</span> bytes,{' '}
      {formatPercent(BigInt(fullest.size), bytesLimit)}% of the{' '}
      {maxBlockBytes.toLocaleString('en-US')}-byte limit ·{' '}
      {most ? (
        <>
          most work:{' '}
          <span className="text-text-bright">{BigInt(most.cycles!).toLocaleString('en-US')}</span>{' '}
          cycles, {formatPercent(BigInt(most.cycles!), cyclesLimit)}% of the{' '}
          {cyclesLimit.toLocaleString('en-US')}-cycle limit
        </>
      ) : (
        `cycles not run yet for these blocks; the limit is ${cyclesLimit.toLocaleString('en-US')}`
      )}
    </p>
  );
}
