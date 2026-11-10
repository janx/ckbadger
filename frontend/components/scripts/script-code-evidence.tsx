import { Fragment, useState } from 'react';
import { DetailSection } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
import { AppLink } from '@/components/ui/app-link';
import { Chart } from '@/components/ui/chart';
import type {
  ScriptCodeClassTotalApi,
  ScriptCustodyApi,
  ScriptHistoryMarkerApi,
  StackedAreaChartResponse,
} from '@/lib/api';
import { formatCkbAmount, truncateHash } from '@/lib/utils';
import { ExactCkb } from '@/components/activity-event-row';
import { formatPercent } from '@/lib/ckb-decimal';
import { formatCount } from '@/lib/format-count';
import { CODE_CLASS_COLORS } from '@/lib/design-tokens';
import { formatExactDecimal } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';
import { partitionSpans } from '@/lib/partition-geometry';
import { useElementWidth } from '@/hooks/useElementWidth';
import { cn } from '@/lib/utils';

const CLASS_LABELS: Record<ScriptCodeClassTotalApi['class'], string> = {
  fixed: 'Immutable · pinned by data hash',
  unspendable: 'Immutable · code cell unspendable',
  one_key: 'One key can replace',
  multisig: 'A multisig can replace',
  unsupported: 'Holder lock not recognised',
  retired: 'Retired reference',
  mixed: 'Multiple code cell holders',
  ambiguous: 'Different live bytecode',
};

/** Display order of the "Who can change the code" bar and legend: immutable first. */
const CLASS_ORDER: readonly ScriptCodeClassTotalApi['class'][] = [
  'fixed',
  'unspendable',
  'multisig',
  'one_key',
  'unsupported',
  'mixed',
  'ambiguous',
  'retired',
];

export function scriptClassLabel(value: ScriptCodeClassTotalApi['class']): string {
  return CLASS_LABELS[value];
}

/** API classes whose code nobody can replace (the board's "Immutable"). */
export function isImmutableClass(value: ScriptCodeClassTotalApi['class']): boolean {
  return value === 'fixed' || value === 'unspendable';
}

/**
 * What "deprecated" means for a script, the title of its warn tag on /scripts and the script page:
 * the registry the labels come from marks it superseded.
 */
export const DEPRECATED_TITLE =
  'Deprecated: the script registry marks this script as superseded. Cells that already use it still run; do not use it for new cells.';

/**
 * Why a script's code cannot change, in the Code column's words ("code cell unspendable · pinned
 * by data hash"), the unspendable code cell first: the "immutable" tag's title on the scripts
 * list and the script page.
 */
export function immutableHow(classes: readonly ScriptCodeClassTotalApi['class'][]): string[] {
  return classes
    .filter(isImmutableClass)
    .sort((a, b) => Number(b === 'unspendable') - Number(a === 'unspendable'))
    .map((c) => (c === 'fixed' ? 'pinned by data hash' : 'code cell unspendable'));
}

/**
 * `part / total` as the Scripts board writes a share: one decimal from 10 %, two from 0.1 %,
 * two significant digits below that, rounded half-up and never a false `0.00%`.
 */
export function scriptShare(part: bigint, total: bigint): string {
  if (part === BigInt(0)) return '0%';
  const hundredTimes = part * BigInt(100);
  if (hundredTimes >= total * BigInt(10)) return `${formatPercent(part, total, 1)}%`;
  if (hundredTimes * BigInt(10) >= total) return `${formatPercent(part, total, 2)}%`;
  let decimals = 2;
  while (hundredTimes * BigInt(10) ** BigInt(decimals) < total * BigInt(10)) decimals++;
  return `${formatPercent(part, total, decimals).replace(/0+$/, '').replace(/\.$/, '')}%`;
}

export function CodeClassSwatch({
  value,
  className,
}: {
  value: ScriptCodeClassTotalApi['class'];
  className?: string;
}) {
  const swatch = CODE_CLASS_COLORS[value];
  return (
    <span
      aria-hidden="true"
      className={className ?? 'inline-block h-2 w-3.5 shrink-0 rounded-sm'}
      style={{ backgroundColor: swatch.fill, boxShadow: `inset 0 0 0 1px ${swatch.edge}` }}
    />
  );
}

/** What the sentence says each group of classes does to the code. */
function classDoes(value: ScriptCodeClassTotalApi['class']): string {
  if (isImmutableClass(value)) return 'runs code no one can change';
  if (value === 'multisig') return 'runs code a multisig can replace';
  if (value === 'one_key') return 'runs code a single key can replace';
  return `sits under code marked "${scriptClassLabel(value).toLowerCase()}"`;
}

function joinClauses(clauses: readonly string[]): string {
  if (clauses.length <= 1) return clauses.join('');
  return `${clauses.slice(0, -1).join(', ')} and ${clauses[clauses.length - 1]}`;
}

/**
 * The list summary's classes, checked and in display order, with the exact CKB under them: one
 * path for the hero's bar, its legend row and the tier-2 sentence. `held` are the classes whose
 * lock scripts hold CKB (the bar's segments).
 */
export function codeClassTotals(classes: readonly ScriptCodeClassTotalApi[]): {
  ordered: ScriptCodeClassTotalApi[];
  held: ScriptCodeClassTotalApi[];
  total: bigint;
} {
  const seen = new Set<string>();
  for (const row of classes) {
    if (!(row.class in CLASS_LABELS)) throw new Error(`Unknown script code class ${row.class}`);
    if (row.familyCount === undefined)
      throw new Error(`Script code class ${row.class} summary has no familyCount`);
    if (seen.has(row.class)) throw new Error(`Script code class ${row.class} appears twice`);
    seen.add(row.class);
  }
  const ordered = CLASS_ORDER.flatMap((value) => classes.filter((row) => row.class === value));
  const total = ordered.reduce((sum, row) => sum + BigInt(row.lockOwnedCapacitySum), BigInt(0));
  const held = ordered.filter((row) => BigInt(row.lockOwnedCapacitySum) > BigInt(0));
  return { ordered, held, total };
}

/** The in-segment words: 12 px with 10 px padding each side (the DAO concentration bar's). */
const SEGMENT_LABEL_PX = 12;
const SEGMENT_LABEL_PAD = 10;
/** A monospace advance: whole pixels where the browser snaps them (Chromium), else 0.6 em. */
const advance = (px: number) => Math.max(Math.round(0.6 * px), 0.6 * px);
/** Words inside a segment: dark on the light grey fill, light on the darker ones. */
const SEGMENT_INK: Partial<Record<ScriptCodeClassTotalApi['class'], string>> = {
  fixed: 'text-base-bg',
};

/**
 * "Who can change the code" as the Scripts index's picture (hierarchy v3, C1, R12, R15): a 28 px
 * bar of the CKB under lock scripts by code class, each segment's name and share written inside it
 * where they fit (never an exact amount). Nothing is drawn while no lock script holds CKB.
 */
export function ScriptCodeClassBar({ classes }: { classes: readonly ScriptCodeClassTotalApi[] }) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const { held, total } = codeClassTotals(classes);
  if (total === BigInt(0)) return null;
  const spans = partitionSpans(
    held.map((row) => BigInt(row.lockOwnedCapacitySum)),
    total,
    0,
    100,
    'Who can change the code'
  );
  const words = (row: ScriptCodeClassTotalApi) =>
    `${scriptClassLabel(row.class)} ${scriptShare(BigInt(row.lockOwnedCapacitySum), total)}`;
  const fits = (text: string, i: number) =>
    width !== null &&
    (width * spans[i].width) / 100 >=
      text.length * advance(SEGMENT_LABEL_PX) + 2 * SEGMENT_LABEL_PAD;
  return (
    <div
      ref={ref}
      role="img"
      aria-label="CKB under lock scripts by who can change their code"
      className="text-small flex h-[28px] w-full min-w-0 gap-[2px] overflow-hidden rounded tabular-nums"
    >
      {held.map((row, i) => {
        const inside = fits(words(row), i) ? words(row) : '';
        return (
          <span
            key={row.class}
            data-code-class-segment={row.class}
            title={`${scriptClassLabel(row.class)}: ${scriptShare(BigInt(row.lockOwnedCapacitySum), total)}`}
            className={cn(
              'flex min-w-[3px] items-center overflow-hidden whitespace-nowrap rounded',
              inside && 'px-2.5',
              SEGMENT_INK[row.class] ?? 'text-text-bright'
            )}
            style={{
              width: `${spans[i].width}%`,
              backgroundColor: CODE_CLASS_COLORS[row.class].fill,
              boxShadow: `inset 0 0 0 1px ${CODE_CLASS_COLORS[row.class].edge}`,
            }}
          >
            {inside && <span>{inside}</span>}
          </span>
        );
      })}
    </div>
  );
}

/**
 * The bar's key in one quiet row (R12, R15): every drawn segment's swatch, name and share, since
 * the narrow segments cannot hold their words. The exact amounts are tier 2's (the summary).
 */
export function ScriptCodeClassLegend({
  classes,
}: {
  classes: readonly ScriptCodeClassTotalApi[];
}) {
  const { held, total } = codeClassTotals(classes);
  if (total === BigInt(0)) return null;
  return (
    <ul
      aria-label="Who can change the code, in shares"
      className="text-text-dim text-small flex flex-wrap gap-x-6 gap-y-1"
    >
      {held.map((row) => (
        <li key={row.class} className="inline-flex items-center gap-2 whitespace-nowrap">
          <CodeClassSwatch value={row.class} />
          <span>{scriptClassLabel(row.class)}</span>{' '}
          <span className="text-text tabular-nums">
            {scriptShare(BigInt(row.lockOwnedCapacitySum), total)}
          </span>
        </li>
      ))}
    </ul>
  );
}

/**
 * "Who can change the code" in words and exact amounts — the Scripts index's tier-2 evidence since
 * hierarchy v3 (the bar is the hero's picture): the sentence with the shares, every class's exact
 * CKB and script count, and what "unspendable" means.
 */
export function ScriptCodeClassSummary({
  classes,
  lockScriptCount,
  title = 'Who can change the code',
}: {
  classes: ScriptCodeClassTotalApi[];
  /** Lock scripts the totals cover (the list's role count). */
  lockScriptCount: number;
  title?: string;
}) {
  const { ordered, held, total } = codeClassTotals(classes);
  // Immutable classes share one clause; every other class keeps its own.
  // A family can hold references of two immutable classes, so their family counts do not add:
  // the immutable clause states no count; every other clause is one class and states its own.
  const groups: { does: string; capacity: bigint; families: number | null }[] = [];
  for (const row of held) {
    const does = classDoes(row.class);
    const existing = groups.find((group) => group.does === does);
    if (existing) {
      existing.capacity += BigInt(row.lockOwnedCapacitySum);
      existing.families = null;
    } else
      groups.push({
        does,
        capacity: BigInt(row.lockOwnedCapacitySum),
        families: isImmutableClass(row.class) ? null : row.familyCount!,
      });
  }
  return (
    <DetailSection title={title}>
      {total > BigInt(0) && (
        <Prose className="mb-3">
          <p data-testid="code-class-sentence">
            Of the <ExactCkb shannons={total} signed={false} tone="plain" /> under these{' '}
            {lockScriptCount.toLocaleString('en-US')} lock scripts,{' '}
            {joinClauses(
              groups.map(
                (group) =>
                  `${scriptShare(group.capacity, total)} ${group.does}${group.families === null ? '' : ` (${formatCount(group.families, 'script')})`}`
              )
            )}
            .
          </p>
        </Prose>
      )}
      <ul
        aria-label="Code classes"
        className="text-text-dim text-meta flex flex-wrap gap-x-7 gap-y-1.5"
      >
        {ordered.map((row) => (
          <li key={row.class} className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <CodeClassSwatch value={row.class} />
            <span>{scriptClassLabel(row.class)}</span>·
            <ExactCkb
              shannons={BigInt(row.lockOwnedCapacitySum)}
              signed={false}
              tone="plain"
            />· {formatCount(row.familyCount!, 'script')}
          </li>
        ))}
      </ul>
      {classes.some((row) => row.class === 'unspendable') && (
        <p data-testid="unspendable-note" className="text-text-dim text-body mt-3 max-w-[640px]">
          Unspendable: the code cell sits under a lock no one can unlock — the all-zero lock, or a
          secp256k1 lock over 20 zero bytes, which no known key matches — so its code is effectively
          immutable.
        </p>
      )}
    </DetailSection>
  );
}

/** A code cell the API names as `0x<tx hash>:<output index>`, opening its cell page. */
function CodeCellLink({ outpoint }: { outpoint: string }) {
  const at = outpoint.lastIndexOf(':');
  if (at < 1 || at === outpoint.length - 1)
    throw new Error(`Code cell out point "${outpoint}" is not "0x<tx hash>:<output index>"`);
  return (
    <AppLink
      href={`/cell/${outpoint.slice(0, at)}-${outpoint.slice(at + 1)}`}
      className="text-emphasis hover:underline"
    >
      {outpoint}
    </AppLink>
  );
}

export function ScriptCustodyEvidence({ custody }: { custody: ScriptCustodyApi }) {
  const signer = custody.verifiedByTx;
  return (
    <div className="text-small min-w-0">
      <div className="text-text-bright">{scriptClassLabel(custody.class)}</div>
      {custody.class === 'multisig' && custody.witnessState === 'verified' && (
        <div className="text-text-dim mt-1">
          {custody.threshold} of {custody.keyCount} keys · witness configuration matches the code
          cell lock arguments
        </div>
      )}
      {custody.witnessState === 'unavailable' && custody.class === 'multisig' && (
        <div className="text-text-dim mt-1">Signing configuration unavailable</div>
      )}
      {custody.witnessState === 'never_signed' && (
        <div className="text-text-dim mt-1">
          Never signed: every indexed cell under this exact holder lock remains live.
        </div>
      )}
      {custody.lockArgs && (
        <div className="text-text-dim text-meta mt-1 break-all font-mono" title={custody.lockArgs}>
          Lock args {custody.lockArgs}
        </div>
      )}
      {custody.keyHashes.length > 0 && (
        <details className="text-text-dim mt-1">
          <summary className="cursor-pointer">
            {custody.keyHashes.length} committed key hashes
          </summary>
          <ul className="text-meta mt-1 space-y-1 break-all font-mono">
            {custody.keyHashes.map((hash) => (
              <li key={hash}>{hash}</li>
            ))}
          </ul>
        </details>
      )}
      {signer && (
        <div className="text-text-dim mt-1 break-all">
          This holder signed to spend the code cell in{' '}
          <AppLink href={`/tx/${signer}`} className="text-emphasis hover:underline">
            {truncateHash(signer, 12, 8)}
          </AppLink>
        </div>
      )}
      {custody.owners.length > 1 && (
        <ul className="text-text-dim mt-2 space-y-1">
          {custody.owners.map((owner) => (
            <li
              key={`${owner.lockCodeHash}:${owner.lockHashType}:${owner.lockArgs}`}
              className="break-all"
            >
              {scriptClassLabel(owner.class)} · {owner.lockHashType} {owner.lockCodeHash} · args{' '}
              {owner.lockArgs} · {owner.codeCells.length} code cell
              {owner.codeCells.length === 1 ? '' : 's'}:{' '}
              {owner.codeCells.map((outpoint, i) => (
                <Fragment key={outpoint}>
                  {i > 0 && ', '}
                  <CodeCellLink outpoint={outpoint} />
                </Fragment>
              ))}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ScriptCodeHistory({ markers }: { markers: ScriptHistoryMarkerApi[] }) {
  return (
    <DetailSection title="Code history">
      {markers.length === 0 ? (
        <p className="text-text-dim text-small">
          No observed code cell events for registered references.
        </p>
      ) : (
        <ol className="border-base-border space-y-0 border-l pl-4">
          {markers.map((marker) => (
            <li
              key={`${marker.referenceHash}:${marker.hashType}:${marker.codeCellTxHash}:${marker.codeCellOutputIndex}:${marker.event}`}
              className="border-base-border border-b py-3 last:border-b-0"
            >
              <div className="text-small flex flex-wrap items-baseline justify-between gap-2">
                <span className="text-text-bright capitalize">{marker.event}</span>
                <AppLink
                  href={`/blocks/${marker.blockNumber}`}
                  className="text-emphasis hover:underline"
                >
                  Block {formatBlockNumber(marker.blockNumber)}
                </AppLink>
              </div>
              <div className="text-text-dim text-meta mt-1 break-all font-mono">
                {marker.hashType} reference {truncateHash(marker.referenceHash, 16, 8)} · version{' '}
                {marker.versionHash ? truncateHash(marker.versionHash, 16, 8) : 'unresolved'}
              </div>
              <div className="text-text-dim text-meta mt-1">
                Chart day {marker.indexedDay} · code cell holder:{' '}
                {scriptClassLabel(marker.custody.class)}
              </div>
              <AppLink
                href={`/tx/${marker.eventTxHash}`}
                className="text-emphasis text-meta hover:underline"
              >
                Event transaction {truncateHash(marker.eventTxHash, 12, 8)}
              </AppLink>
            </li>
          ))}
        </ol>
      )}
    </DetailSection>
  );
}

export function ScriptCapacityEvidenceChart({
  chart,
  markers,
}: {
  chart: StackedAreaChartResponse;
  markers: ScriptHistoryMarkerApi[];
}) {
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const key = (marker: ScriptHistoryMarkerApi) =>
    `${marker.eventTxHash}:${marker.event}:${marker.codeCellTxHash}:${marker.codeCellOutputIndex}:${marker.hashType}:${marker.referenceHash}`;
  const selected = markers.find((marker) => key(marker) === selectedKey) ?? markers.at(-1);
  const consumedCandidates =
    selected?.event === 'deployed'
      ? markers.filter(
          (marker) =>
            marker.event === 'consumed' &&
            marker.eventTxHash === selected.eventTxHash &&
            marker.referenceHash === selected.referenceHash &&
            marker.hashType === selected.hashType
        )
      : [];
  const consumedInUpgrade = consumedCandidates.length === 1 ? consumedCandidates[0] : undefined;
  const retainedKeys =
    consumedInUpgrade &&
    selected &&
    consumedInUpgrade.custody.witnessState === 'verified' &&
    selected.custody.witnessState === 'verified'
      ? selected.custody.keyHashes.filter((hash) =>
          consumedInUpgrade.custody.keyHashes.includes(hash)
        )
      : null;
  const dateIndex = new Map(chart.data.map((point, index) => [point.date, index]));
  const displaySeries = chart.series.map((series) => ({
    ...series,
    label: series.key === 'used' ? 'Occupied' : series.key === 'unused' ? 'Free' : series.label,
    color:
      series.key === 'used'
        ? 'var(--c-data-framing)'
        : series.key === 'unused'
          ? 'var(--c-jade-dim)'
          : series.color,
  }));
  const custodyBands = markers
    .filter((marker) => marker.event === 'deployed')
    .flatMap((deployed) => {
      const end = markers.find(
        (marker) =>
          marker.event === 'consumed' &&
          marker.referenceHash === deployed.referenceHash &&
          marker.hashType === deployed.hashType &&
          marker.codeCellTxHash === deployed.codeCellTxHash &&
          marker.codeCellOutputIndex === deployed.codeCellOutputIndex
      );
      const firstVisible = chart.data.findIndex((day) => day.date >= deployed.indexedDay);
      const start = firstVisible < 0 ? chart.data.length : firstVisible;
      if (end && end.indexedDay < chart.data[0].date) return [];
      const endVisible = end ? chart.data.findIndex((day) => day.date >= end.indexedDay) : -1;
      const stop = endVisible < 0 ? chart.data.length - 1 : endVisible;
      if (start >= chart.data.length || stop < start) return [];
      const scale = Math.max(chart.data.length - 1, 1);
      return [
        {
          deployed,
          start: (start / scale) * 100,
          width: ((stop - start) / scale) * 100,
          retired: Boolean(end),
        },
      ];
    });
  const chartMarkerGroups = new Map<number, ScriptHistoryMarkerApi[]>();
  for (const marker of markers) {
    const index = dateIndex.get(marker.indexedDay);
    if (index !== undefined)
      chartMarkerGroups.set(index, [...(chartMarkerGroups.get(index) ?? []), marker]);
  }
  const chartMarkers = [...chartMarkerGroups].map(([index, dayMarkers]) => ({
    index,
    label: `code · ${dayMarkers.length} event${dayMarkers.length === 1 ? '' : 's'}`,
    plotLabel: '◆',
    href: `/blocks/${dayMarkers[0].blockNumber}`,
  }));
  return (
    <div className="min-w-0">
      <Chart
        title="CKB in this script’s cells"
        subtitle="Occupied and free capacity, indexed chain day"
        data={chart.data.map((point) => ({
          label: point.date,
          values: Object.fromEntries(
            Object.entries(point.values).map(([series, shannons]) => [
              series,
              formatCkbAmount(shannons).full.replaceAll(',', ''),
            ])
          ),
        }))}
        series={displaySeries}
        unit="CKB"
        kind="stacked"
        height={240}
        plotLeft={88}
        markers={chartMarkers}
        formatValue={(value) => formatExactDecimal(value, 'script cell capacity')}
      />
      {chart.data.length > 0 && (
        <div
          aria-label="Custodian band aligned with capacity chart days"
          className="text-meta mt-3"
        >
          <div className="text-text-dim mb-1">
            Custodian track · blank intervals are retired gaps
          </div>
          <div className="space-y-1 pl-[88px] pr-[10px]">
            {custodyBands.map(({ deployed, start, width, retired }) => (
              <div
                key={key(deployed)}
                className="border-base-border relative h-5 border-b border-dashed"
                title={`${deployed.hashType} ${deployed.referenceHash} · ${scriptClassLabel(deployed.custody.class)} · ${deployed.indexedDay}${retired ? ' to retirement' : ' to latest chart day'}`}
              >
                <div
                  className="bg-emphasis/40 absolute top-1 h-3 min-w-[3px] rounded-sm"
                  style={{ left: `${start}%`, width: `${width}%` }}
                />
              </div>
            ))}
          </div>
        </div>
      )}
      {markers.length > 0 && (
        <div className="border-base-border mt-4 border-t pt-3">
          <div className="text-text-dim text-meta mb-2">
            Code and custodian events on the same indexed calendar as the chart
          </div>
          <div className="flex max-w-full gap-2 overflow-x-auto pb-2">
            {/* One chip an event: the day · event · block selects it; the out point opens the code cell. */}
            {markers.map((marker) => (
              <span
                key={key(marker)}
                className="border-base-border text-text-dim text-meta inline-flex shrink-0 items-center gap-1 rounded border px-2 py-1"
              >
                <button
                  type="button"
                  onClick={() => setSelectedKey(key(marker))}
                  aria-pressed={selected === marker}
                  className="hover:text-text-bright"
                >
                  {marker.indexedDay} · {marker.event} · {formatBlockNumber(marker.blockNumber)}
                </button>
                <span aria-hidden="true">·</span>
                <AppLink
                  href={`/cell/${marker.codeCellTxHash}-${marker.codeCellOutputIndex}`}
                  className="hover:text-text-bright hover:underline"
                >
                  {truncateHash(marker.codeCellTxHash, 8, 6)}:{marker.codeCellOutputIndex}
                </AppLink>
              </span>
            ))}
          </div>
          {selected && (
            <div className="text-small mt-3 grid gap-3 sm:grid-cols-2">
              <div>
                <div className="text-text-dim">
                  {selected.event === 'deployed' ? 'New code cell holder' : 'Old code cell holder'}
                </div>
                <ScriptCustodyEvidence custody={selected.custody} />
              </div>
              {consumedInUpgrade && (
                <div>
                  <div className="text-text-dim">
                    Previous code cell holder · signed this upgrade
                  </div>
                  <ScriptCustodyEvidence custody={consumedInUpgrade.custody} />
                </div>
              )}
              {consumedCandidates.length > 1 && (
                <div className="text-text-dim sm:col-span-2">
                  Multiple previous code cells under this reference were consumed in the same
                  transaction; previous holder is ambiguous.
                </div>
              )}
              {retainedKeys && (
                <div className="text-text-dim sm:col-span-2">
                  Retained {retainedKeys.length} of {consumedInUpgrade!.custody.keyHashes.length}{' '}
                  previous keys; replaced{' '}
                  {consumedInUpgrade!.custody.keyHashes.length - retainedKeys.length}.
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
