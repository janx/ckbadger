import { useState } from 'react';
import { DetailSection } from '@/components/ui/detail-layout';
import { AppLink } from '@/components/ui/app-link';
import { Chart } from '@/components/ui/chart';
import type {
  ScriptCodeClassTotalApi,
  ScriptCustodyApi,
  ScriptHistoryMarkerApi,
  StackedAreaChartResponse,
} from '@/lib/api';
import { formatCkbAmount, formatCkbCompact, truncateHash } from '@/lib/utils';

const CLASS_LABELS: Record<ScriptCodeClassTotalApi['class'], string> = {
  fixed: 'Fixed bytecode',
  unspendable: 'Unspendable code cell',
  one_key: 'One key can replace code',
  multisig: 'Multisig can replace code',
  unsupported: 'Custody unsupported',
  retired: 'Retired reference',
  mixed: 'Multiple code cell holders',
  ambiguous: 'Different live bytecode',
};

export function scriptClassLabel(value: ScriptCodeClassTotalApi['class']): string {
  return CLASS_LABELS[value];
}

function classPercent(capacity: bigint, total: bigint): string {
  if (total === BigInt(0)) return '0%';
  const hundredths = (capacity * BigInt(10000)) / total;
  return `${hundredths / BigInt(100)}.${String(hundredths % BigInt(100)).padStart(2, '0')}%`;
}

export function ScriptCodeClassSummary({
  classes,
  title = 'Who can change the code',
}: {
  classes: ScriptCodeClassTotalApi[];
  title?: string;
}) {
  const total = classes.reduce((sum, row) => sum + BigInt(row.lockOwnedCapacitySum), BigInt(0));
  return (
    <DetailSection title={title}>
      <p className="text-text-dim mb-4 text-xs">
        Live CKB held by lock scripts in each code class. A fixed data reference pins bytecode; a
        Type ID reference can change as its code cell changes.
      </p>
      {total > BigInt(0) && (
        <div
          role="img"
          aria-label="Live lock capacity distribution by code class"
          className="bg-base-border mb-4 flex h-3 w-full overflow-hidden rounded"
        >
          {classes
            .filter((row) => BigInt(row.lockOwnedCapacitySum) > BigInt(0))
            .map((row, index) => (
              <div
                key={row.class}
                title={`${scriptClassLabel(row.class)}: ${classPercent(BigInt(row.lockOwnedCapacitySum), total)}`}
                className={index % 2 === 0 ? 'bg-emphasis' : 'bg-text-dim'}
                style={{
                  width: `${Number((BigInt(row.lockOwnedCapacitySum) * BigInt(1000000)) / total) / 10000}%`,
                }}
              />
            ))}
        </div>
      )}
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {classes.map((row) => {
          const capacity = BigInt(row.lockOwnedCapacitySum);
          const compact = formatCkbCompact(row.lockOwnedCapacitySum);
          return (
            <div key={row.class} className="border-base-border min-w-0 border-t pt-3">
              <div className="text-text-bright text-xs">{scriptClassLabel(row.class)}</div>
              <div
                className="text-text-bright mt-1 font-mono tabular-nums"
                title={`${compact.full} CKB`}
              >
                {compact.value} CKB
              </div>
              <div className="text-text-dim mt-1 text-[11px] tabular-nums">
                {classPercent(capacity, total)} · {row.lockCellsCount.toLocaleString()} live cells ·{' '}
                {row.referenceCount.toLocaleString()} references
              </div>
            </div>
          );
        })}
      </div>
    </DetailSection>
  );
}

export function ScriptCustodyEvidence({ custody }: { custody: ScriptCustodyApi }) {
  const signer = custody.verifiedByTx;
  return (
    <div className="min-w-0 text-xs">
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
        <div
          className="text-text-dim mt-1 break-all font-mono text-[11px]"
          title={custody.lockArgs}
        >
          Lock args {custody.lockArgs}
        </div>
      )}
      {custody.keyHashes.length > 0 && (
        <details className="text-text-dim mt-1">
          <summary className="cursor-pointer">
            {custody.keyHashes.length} committed key hashes
          </summary>
          <ul className="mt-1 space-y-1 break-all font-mono text-[11px]">
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
              {owner.codeCells.length === 1 ? '' : 's'}: {owner.codeCells.join(', ')}
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
        <p className="text-text-dim text-xs">
          No observed code cell events for registered references.
        </p>
      ) : (
        <ol className="border-base-border space-y-0 border-l pl-4">
          {markers.map((marker) => (
            <li
              key={`${marker.referenceHash}:${marker.hashType}:${marker.codeCellTxHash}:${marker.codeCellOutputIndex}:${marker.event}`}
              className="border-base-border border-b py-3 last:border-b-0"
            >
              <div className="flex flex-wrap items-baseline justify-between gap-2 text-xs">
                <span className="text-text-bright capitalize">{marker.event}</span>
                <AppLink
                  href={`/blocks/${marker.blockNumber}`}
                  className="text-emphasis hover:underline"
                >
                  Block #{marker.blockNumber.toLocaleString()}
                </AppLink>
              </div>
              <div className="text-text-dim mt-1 break-all font-mono text-[11px]">
                {marker.hashType} reference {truncateHash(marker.referenceHash, 16, 8)} · version{' '}
                {marker.versionHash ? truncateHash(marker.versionHash, 16, 8) : 'unresolved'}
              </div>
              <div className="text-text-dim mt-1 text-[11px]">
                Chart day {marker.indexedDay} · code cell holder:{' '}
                {scriptClassLabel(marker.custody.class)}
              </div>
              <AppLink
                href={`/tx/${marker.eventTxHash}`}
                className="text-emphasis text-[11px] hover:underline"
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
    color: series.key === 'used' ? '#58a5b5' : series.key === 'unused' ? '#39bba6' : series.color,
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
        formatValue={(value) => value}
      />
      {chart.data.length > 0 && (
        <div
          aria-label="Custodian band aligned with capacity chart days"
          className="mt-3 text-[11px]"
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
          <div className="text-text-dim mb-2 text-[11px]">
            Code and custodian events on the same indexed calendar as the chart
          </div>
          <div className="flex max-w-full gap-2 overflow-x-auto pb-2">
            {markers.map((marker) => (
              <button
                key={key(marker)}
                type="button"
                onClick={() => setSelectedKey(key(marker))}
                aria-pressed={selected === marker}
                className="border-base-border text-text-dim hover:text-text-bright shrink-0 rounded border px-2 py-1 text-[11px]"
              >
                {marker.indexedDay} · {marker.event} · #{marker.blockNumber} ·{' '}
                {truncateHash(marker.referenceHash, 8, 6)}:{marker.codeCellOutputIndex}
              </button>
            ))}
          </div>
          {selected && (
            <div className="mt-3 grid gap-3 text-xs sm:grid-cols-2">
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
