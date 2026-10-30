'use client';
import { useState, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { Chart, type ChartMarker } from '@/components/ui/chart';
import { ExactCkb } from '@/components/activity-event-row';
import { ScriptCustodyEvidence, isImmutableClass } from '@/components/scripts/script-code-evidence';
import type {
  ScriptCustodyApi,
  ScriptFamilyDetail,
  ScriptHistoryMarkerApi,
  StackedAreaChartResponse,
} from '@/lib/api';
import { formatExactDecimal } from '@/lib/exact-integer';
import { formatCkbAmount, formatUtc8Date } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { scriptShare } from '@/components/scripts/script-code-evidence';
import { DATA_COLOR, HELD_CAPACITY_COLOR } from '@/lib/design-tokens';
import { deploymentOnDay, holderName, signingText, type CodeHolder } from '@/lib/script-lineage';

const NUMBER_WORDS = ['No', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine'];

function roleNoun(scriptKind: string | null): string {
  return scriptKind === 'lock'
    ? 'lock script'
    : scriptKind === 'type'
      ? 'type script'
      : scriptKind === 'lock+type'
        ? 'lock and type script'
        : 'script';
}

/** "lock script · upgradable": the role and whether anyone can replace the code. */
export function scriptCodeWord(family: ScriptFamilyDetail): string | null {
  const classes = family.codeClasses ?? [];
  if (!classes.length) return null;
  return classes.every((row) => isImmutableClass(row.class)) ? 'immutable' : 'upgradable';
}

export function ScriptHeading({ family }: { family: ScriptFamilyDetail }) {
  const code = scriptCodeWord(family);
  return (
    <span className="text-text-dim text-[13px]">
      {roleNoun(family.scriptKind)}
      {code ? ` · ${code}` : ''}
    </span>
  );
}

function holderSentence(custody: ScriptCustodyApi): string {
  if (custody.class === 'multisig')
    return custody.witnessState === 'never_signed'
      ? 'a multisig that has never signed'
      : `${holderName(custody)}`;
  if (custody.class === 'one_key') return 'a single key';
  if (custody.class === 'unspendable') return 'a lock that can never be unlocked';
  return holderName(custody);
}

/** The board's summary sentence: what the script guards, since when, and who can change it. */
export function ScriptSummarySentence({
  family,
  lineage,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
}) {
  const firstDeployed = Math.min(
    ...family.versions.flatMap((version) => version.deployments.map((d) => d.deployedAt))
  );
  const verb =
    family.scriptKind === 'type'
      ? 'decides what'
      : family.scriptKind === 'lock'
        ? 'decides who can spend'
        : 'guards';
  const code = scriptCodeWord(family);
  const last = lineage?.at(-1);
  return (
    <p data-testid="script-summary" className="max-w-[780px] text-[15px] leading-relaxed">
      {family.name} {verb}{' '}
      <span className="text-text-bright">{family.liveCellsCount.toLocaleString('en-US')}</span> live
      cells holding{' '}
      <span className="text-text-bright">
        <ExactCkb shannons={BigInt(family.ownedCapacitySum)} signed={false} tone="plain" />
      </span>
      ; <span className="text-text-bright">{family.cellsCount.toLocaleString('en-US')}</span> cells
      have used it
      {Number.isFinite(firstDeployed) ? ` since ${formatUtc8Date(firstDeployed)}` : ''}.
      {code === 'immutable' && ' Its code is immutable: no one can replace it.'}
      {code === 'upgradable' && lineage && last && lineage.length > 1 && (
        <>
          {' '}
          Its code is upgradable: it has been replaced{' '}
          <span className="text-text-bright">{formatCount(lineage.length - 1, 'time')}</span>, last
          on {formatUtc8Date(last.timestamp)}, and its code cell now sits under{' '}
          {holderSentence(last.custody)}.
        </>
      )}
      {code === 'upgradable' && lineage && lineage.length === 1 && (
        <> Its code is upgradable by {holderSentence(lineage[0].custody)}.</>
      )}
    </p>
  );
}

function RailFact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt className="text-text-dim text-xs">{label}</dt>
      {children}
    </div>
  );
}

export function ScriptFactsRail({
  family,
  lineage,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
}) {
  const capacity = BigInt(family.ownedCapacitySum);
  const occupied = BigInt(family.ownedKnowledgeSum);
  if (occupied > capacity)
    throw new Error(`${family.name}: occupied ${occupied} exceeds capacity ${capacity}`);
  const share = capacity === BigInt(0) ? null : scriptShare(occupied, capacity);
  const code = scriptCodeWord(family);
  const last = lineage?.at(-1);
  const firstDeployed = Math.min(
    ...family.versions.flatMap((version) => version.deployments.map((d) => d.deployedAt))
  );
  return (
    <aside aria-label="Script facts" className="min-w-0 lg:sticky lg:top-4 lg:pt-8">
      <dl className="flex flex-col gap-4">
        <RailFact label="Live cells">
          <dd className="text-text-bright mt-1 text-[18px] font-medium tabular-nums">
            {family.liveCellsCount.toLocaleString('en-US')}
          </dd>
          <dd className="text-text-dim mt-1 text-xs">
            of {family.cellsCount.toLocaleString('en-US')} that ever used it ·{' '}
            <a href="#its-cells" className="hover:text-text-bright">
              list them →
            </a>
          </dd>
        </RailFact>
        <RailFact label="CKB in its cells">
          <dd className="mt-1 text-[15px]">
            <ExactCkb shannons={capacity} signed={false} tone="plain" />
          </dd>
          {share && (
            <dd aria-hidden="true" className="mt-1.5 flex h-1.5 gap-[2px] overflow-hidden rounded">
              <span
                style={{
                  width: `${Number((occupied * BigInt(100000)) / capacity) / 1000}%`,
                  backgroundColor: DATA_COLOR,
                }}
              />
              <span className="flex-1" style={{ backgroundColor: '#2c3348' }} />
            </dd>
          )}
          <dd className="text-text-dim mt-1 text-xs">
            <span style={{ color: DATA_COLOR }}>
              <ExactCkb shannons={occupied} signed={false} tone="plain" />
            </span>{' '}
            occupied{share ? ` · ${share}` : ''}
          </dd>
        </RailFact>
        {code && (
          <RailFact label="Code">
            <dd className="text-text-bright mt-1 text-[15px]">
              {code === 'immutable'
                ? 'Immutable'
                : `Upgradable · ${formatCount(family.versionsCount, 'version')}`}
            </dd>
            {code === 'upgradable' && last && lineage!.length > 1 && (
              <dd className="text-text-dim mt-1 text-xs">
                last replaced {formatUtc8Date(last.timestamp)}
              </dd>
            )}
          </RailFact>
        )}
        {last && (
          <RailFact label="Code cell held by">
            <dd className="mt-1 text-xs">
              <Link
                href={`/cell/${last.codeCellTxHash}-${last.codeCellOutputIndex}`}
                className="text-text hover:text-jade"
              >
                {holderName(last.custody)}
              </Link>
            </dd>
            {last.custody.witnessState === 'never_signed' && (
              <dd className="text-text-dim mt-0.5 text-xs">has never signed</dd>
            )}
          </RailFact>
        )}
        {Number.isFinite(firstDeployed) && (
          <RailFact label="In use since">
            <dd className="text-text-bright mt-1 text-[15px]">{formatUtc8Date(firstDeployed)}</dd>
          </RailFact>
        )}
      </dl>
    </aside>
  );
}

function ckbDecimal(shannons: string): string {
  return formatCkbAmount(shannons).full.replaceAll(',', '');
}

/**
 * History (Script board): CKB in its cells with the occupied part in jade, occupied on its own
 * scale, and the code track (upgrade days, the custodian band) on the same time axis. One hover
 * reading for all of it, including the code version and its holder on that day.
 */
export function ScriptHistory({
  family,
  chart,
  lineage,
  numbers,
  holders,
}: {
  family: ScriptFamilyDetail;
  chart: StackedAreaChartResponse;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  numbers: ReadonlyMap<string, number>;
  holders: readonly CodeHolder[];
}) {
  const [active, setActive] = useState<number | null>(null);
  for (const key of ['used', 'unused'])
    if (!chart.series.some((series) => series.key === key))
      throw new Error(`${family.name} capacity history has no ${key} series`);
  const data = chart.data.map((point) => ({
    label: point.date,
    values: { used: ckbDecimal(point.values.used), unused: ckbDecimal(point.values.unused) },
  }));
  const index = active ?? chart.data.length - 1;
  const point = chart.data[index];
  const used = point ? BigInt(point.values.used) : null;
  const unused = point ? BigInt(point.values.unused) : null;
  const deployment = point && lineage ? deploymentOnDay(lineage, point.date) : null;
  const version = deployment?.versionHash != null ? numbers.get(deployment.versionHash) : undefined;
  const scale = Math.max(chart.data.length - 1, 1);
  const position = (day: string) => {
    const at = chart.data.findIndex((p) => p.date >= day);
    return at < 0 ? null : (at / scale) * 100;
  };
  const upgradeDays = new Map<string, number[]>();
  for (const marker of lineage ?? []) {
    const n = marker.versionHash ? numbers.get(marker.versionHash) : undefined;
    if (n === undefined) continue;
    upgradeDays.set(marker.indexedDay, [...(upgradeDays.get(marker.indexedDay) ?? []), n]);
  }
  const markers: ChartMarker[] = [...upgradeDays].flatMap(([day, versions]) => {
    const at = chart.data.findIndex((p) => p.date === day);
    const deployed = lineage!.find((marker) => marker.indexedDay === day)!;
    return at < 0
      ? []
      : [
          {
            index: at,
            label: versions.map((n) => `v${n}`).join(' · '),
            plotLabel: '',
            href: `/blocks/${deployed.blockNumber}`,
          },
        ];
  });
  const shared = {
    data,
    unit: 'CKB',
    height: 170,
    plotLeft: 88,
    hideCaption: true,
    activeIndex: active,
    onActiveIndexChange: setActive,
    formatValue: (value: string) => formatExactDecimal(value, 'script cell capacity'),
  } as const;
  return (
    <div className="min-w-0">
      <div
        aria-live="polite"
        data-testid="script-history-reading"
        className="border-base-elevated flex min-h-10 flex-wrap items-baseline gap-x-6 gap-y-1.5 border-b pb-2.5 text-xs"
      >
        {point && used !== null && unused !== null ? (
          <>
            <span className="text-text-bright text-[13px]">
              {point.date}
              {active === null && <span className="text-text-dim text-xs"> · last full day</span>}
            </span>
            <span className="flex items-baseline gap-1.5">
              <span className="text-text-dim">CKB held</span>{' '}
              <ExactCkb shannons={used + unused} signed={false} tone="plain" />
            </span>
            <span className="flex items-baseline gap-1.5">
              <span
                aria-hidden="true"
                className="inline-block h-2 w-2 rounded-sm"
                style={{ backgroundColor: DATA_COLOR }}
              />
              <span className="text-text-dim">occupied</span>{' '}
              <span style={{ color: DATA_COLOR }}>
                <ExactCkb shannons={used} signed={false} tone="plain" />
              </span>
            </span>
            <span className="flex items-baseline gap-1.5">
              <span
                aria-hidden="true"
                className="inline-block h-2 w-2 rounded-sm"
                style={{ backgroundColor: HELD_CAPACITY_COLOR }}
              />
              <span className="text-text-dim">free</span>{' '}
              <ExactCkb shannons={unused} signed={false} tone="plain" />
            </span>
            {deployment && (
              <span className="flex items-baseline gap-1.5">
                <span className="text-text-dim">code</span>{' '}
                <span className="text-text-bright">{version ? `v${version}` : 'unresolved'}</span>{' '}
                <span className="text-text-dim">held by {holderName(deployment.custody)}</span>
              </span>
            )}
          </>
        ) : (
          <span className="text-text-dim">No capacity history in this range.</span>
        )}
      </div>
      <p className="text-text-dim mb-1 mt-3 text-[11px]">CKB in {family.name} cells</p>
      <Chart
        {...shared}
        title={`CKB in ${family.name} cells`}
        subtitle="Occupied and free capacity, indexed chain day"
        kind="stacked"
        series={[
          { key: 'used', label: 'Occupied', color: DATA_COLOR },
          { key: 'unused', label: 'Free', color: HELD_CAPACITY_COLOR },
        ]}
        markers={markers}
      />
      <p className="text-text-dim mb-1 mt-4 text-[11px]">Occupied by those cells&apos; own bytes</p>
      <Chart
        {...shared}
        title="Occupied by those cells' own bytes"
        subtitle="Occupied capacity on its own scale"
        kind="area"
        height={110}
        series={[{ key: 'used', label: 'Occupied', color: DATA_COLOR }]}
      />
      {lineage && chart.data.length > 0 && (
        <div aria-label="Code track" className="mt-2 text-[10.5px]">
          <div className="relative h-4 pl-[88px] pr-[10px]">
            <span className="text-text-dim absolute left-0 top-0">code</span>
            <div className="relative h-full">
              {[...upgradeDays].map(([day, versions]) => {
                const at = position(day);
                return at === null ? null : (
                  <span
                    key={day}
                    data-upgrade-day={day}
                    className="text-text-bright absolute top-0 whitespace-nowrap border-l border-dashed border-[#4b5266] pl-1"
                    style={{ left: `${at}%` }}
                  >
                    {versions.map((n) => `v${n}`).join(' · ')}
                  </span>
                );
              })}
            </div>
          </div>
          <div className="relative mt-1 h-10 pl-[88px] pr-[10px]">
            <div className="relative h-full">
              {holders.map((holder) => {
                const start = position(holder.from.indexedDay);
                if (start === null) return null;
                const end = holder.until ? (position(holder.until.indexedDay) ?? 100) : 100;
                return (
                  <div
                    key={holder.lockArgs}
                    data-custodian={holder.lockArgs}
                    className="absolute top-0"
                    style={{ left: `${start}%`, width: `${Math.max(end - start, 0)}%` }}
                  >
                    <div
                      className={`h-2 rounded-sm border ${holder.neverSigned ? 'border-dashed' : ''}`}
                      style={{
                        borderColor: 'rgba(160,168,190,.7)',
                        backgroundColor: holder.neverSigned
                          ? 'transparent'
                          : 'rgba(160,168,190,.22)',
                      }}
                    />
                    <span className="mt-0.5 block truncate text-[#a0a8be]">
                      {holderName(holder.custody)}
                      {signingText(holder) ? ` · ${signingText(holder)}` : ''}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}
      <p className="text-text-dim mt-3 max-w-[780px] text-xs leading-relaxed">
        A cell must hold enough CKB to pay for its own bytes.{' '}
        <span style={{ color: DATA_COLOR }}>Occupied</span> is that cost for every cell{' '}
        {family.name} guards; <span className="text-[#a0a8be]">free</span> is CKB its owners keep
        there.
        {lineage && lineage.length > 1 ? ' Dashed lines mark the days its code was replaced.' : ''}
      </p>
    </div>
  );
}

function versionList(signed: readonly (number | null)[]): string {
  const names = signed.map((n) => (n === null ? 'an unresolved version' : `v${n}`));
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** Keys of `holder` reordered so a key it shares with `previous` sits on the same row. */
function alignedKeys(holder: readonly string[], previous: readonly string[] | null): string[] {
  if (!previous) return [...holder];
  const fresh = holder.filter((key) => !previous.includes(key));
  const rows = previous.map((key) => (holder.includes(key) ? key : (fresh.shift() ?? null)));
  return [...rows.filter((key): key is string => key !== null), ...fresh];
}

/** "Who can change its code": the holders of the code cell, key by key. */
export function ScriptCustodyFigure({
  family,
  lineage,
  holders,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  holders: readonly CodeHolder[];
}) {
  if (!lineage || !holders.length) {
    const classes = family.codeClasses ?? [];
    if (classes.length && classes.every((row) => isImmutableClass(row.class)))
      return (
        <p className="max-w-[780px] text-[13px] leading-relaxed">
          No one. {family.name} cells name their code by its data hash, or through a code cell that
          can never be spent, so the code they run cannot be replaced.
        </p>
      );
    return (
      <div className="space-y-3 text-xs">
        {(family.references ?? []).map((reference) => (
          <div key={`${reference.hashType}:${reference.referenceHash}`}>
            <div className="text-text-dim mb-1 break-all">
              {reference.hashType} · {reference.referenceHash}
            </div>
            {reference.custody ? (
              <ScriptCustodyEvidence custody={reference.custody} />
            ) : (
              <span className="text-text-dim">Custody evidence unavailable</span>
            )}
          </div>
        ))}
      </div>
    );
  }
  const keyRows: string[][] = [];
  holders.forEach((holder, i) => {
    keyRows.push(
      holder.signing ? alignedKeys(holder.signing.keyHashes, i > 0 ? keyRows[i - 1] : null) : []
    );
  });
  const kept = (i: number, key: string) =>
    (i > 0 && keyRows[i - 1].includes(key)) ||
    (i + 1 < keyRows.length && keyRows[i + 1].includes(key));
  const allMultisig = holders.every((holder) => holder.custody.class === 'multisig');
  const handovers = holders.slice(1).flatMap((holder, i) => {
    const before = keyRows[i];
    const after = keyRows[i + 1];
    if (!before.length || !after.length) return [];
    const common = after.filter((key) => before.includes(key)).length;
    return [{ day: formatUtc8Date(holder.from.timestamp), common, total: before.length }];
  });
  return (
    <div className="min-w-0 space-y-4">
      <p className="max-w-[780px] text-[13px] leading-relaxed">
        {family.name} cells point at their code through one code cell&apos;s type id, not at a fixed
        binary. Whoever can spend that cell can put new code in it, and every {family.name} cell
        runs the new code from the next block. {NUMBER_WORDS[holders.length] ?? holders.length}{' '}
        {allMultisig
          ? holders.length === 1
            ? 'multisig has'
            : 'multisigs have'
          : holders.length === 1
            ? 'holder has'
            : 'holders have'}{' '}
        held it.
      </p>
      <ol
        aria-label="Code cell holders"
        className="grid grid-cols-1 gap-x-6 gap-y-6 md:[grid-template-columns:repeat(var(--holders),minmax(0,1fr))]"
        style={{ ['--holders' as string]: holders.length }}
      >
        {holders.map((holder, i) => (
          <li key={holder.lockArgs} data-holder={holder.lockArgs} className="min-w-0 text-xs">
            <div className="text-text-dim text-[11px]">
              {holder.until
                ? `${formatUtc8Date(holder.from.timestamp)} → ${formatUtc8Date(holder.until.timestamp)}`
                : `since ${formatUtc8Date(holder.from.timestamp)}`}
            </div>
            <div className="text-text-bright mt-1 text-[13.5px]">
              {i > 0 && (
                <span aria-hidden="true" className="text-[#4b5266]">
                  →{' '}
                </span>
              )}
              {holderName(holder.custody)}
            </div>
            {signingText(holder) && (
              <div className={holder.neverSigned ? 'text-text-dim mt-1' : 'mt-1 text-[#a0a8be]'}>
                {signingText(holder)}
              </div>
            )}
            <div className="text-text-dim mt-0.5 text-[11px]">
              {holder.signed.length
                ? `signed ${versionList(holder.signed)}`
                : holder.neverSigned
                  ? 'has never signed'
                  : 'no upgrade signed'}
            </div>
            {keyRows[i].length > 0 ? (
              <ul aria-label={`Keys of ${holderName(holder.custody)}`} className="mt-3 space-y-1.5">
                {keyRows[i].map((key) => (
                  <li
                    key={key}
                    data-kept={kept(i, key)}
                    className="flex items-center gap-2 text-[11px]"
                  >
                    <svg width="8" height="8" aria-hidden="true" className="shrink-0">
                      <circle
                        cx="4"
                        cy="4"
                        r="3.4"
                        fill={kept(i, key) ? '#a0a8be' : '#0c0e15'}
                        stroke="#a0a8be"
                      />
                    </svg>
                    <span className={kept(i, key) ? 'text-text-bright' : 'text-text-dim'}>
                      {key.slice(0, 14)}…
                    </span>
                  </li>
                ))}
              </ul>
            ) : holder.neverSigned ? (
              <div className="mt-3 rounded border border-dashed border-[#343c50] p-3 text-[11.5px] text-[#a0a8be]">
                Its keys and threshold appear on chain the first time it signs.
              </div>
            ) : null}
          </li>
        ))}
      </ol>
      {handovers.map((handover) => (
        <p key={handover.day} className="text-text-dim text-[11px]">
          ● kept across the handover on {handover.day}: {handover.common} of {handover.total} keys ·
          ○ replaced: {handover.total - handover.common} of {handover.total}
        </p>
      ))}
    </div>
  );
}

/** "Use it in a transaction": the reference to put in a script, and the cell dep that serves it. */
export function ScriptUseInTransaction({ family }: { family: ScriptFamilyDetail }) {
  const references = family.references ?? [];
  if (!references.length) return null;
  return (
    <div className="space-y-5">
      {references.map((reference) => {
        const version = family.versions.find(
          (candidate) => candidate.versionHash === reference.currentVersionHash
        );
        const live =
          version?.deployments.filter((deployment) => deployment.status === 'live') ?? [];
        return (
          <dl
            key={`${reference.hashType}:${reference.referenceHash}`}
            className="grid gap-x-10 gap-y-3 text-xs md:grid-cols-3"
          >
            <div className="min-w-0">
              <dt className="text-text-dim">Code hash · hash type</dt>
              <dd className="mt-0.5 [overflow-wrap:anywhere]">{reference.referenceHash}</dd>
              <dd className="text-text-dim mt-0.5">
                {reference.hashType} ·{' '}
                {reference.hashType === 'type' ? 'follows every upgrade' : 'pins this exact binary'}
              </dd>
            </div>
            <div className="min-w-0">
              <dt className="text-text-dim">Cell dep · dep type code</dt>
              {live.length ? (
                live.map((deployment) => (
                  <dd
                    key={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                    className="mt-0.5 [overflow-wrap:anywhere]"
                  >
                    <Link
                      href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                      className="text-text hover:text-jade"
                    >
                      {deployment.codeCellTxHash}:{deployment.codeCellOutputIndex}
                    </Link>
                  </dd>
                ))
              ) : (
                <dd className="text-text-dim mt-0.5">
                  {reference.currentVersionHash
                    ? 'No live code cell for the current version'
                    : 'Retired: no current code cell'}
                </dd>
              )}
              {live.length > 0 && (
                <dd className="text-text-dim mt-0.5">the live code cell of the current version</dd>
              )}
            </div>
            {reference.hashType === 'type' && version && (
              <div className="min-w-0">
                <dt className="text-text-dim">Pin this exact binary instead</dt>
                <dd className="mt-0.5 [overflow-wrap:anywhere]">{version.versionHash}</dd>
                <dd className="text-text-dim mt-0.5">
                  its data hash, with hash type data1 or data2; an upgrade will not reach cells that
                  pin it
                </dd>
              </div>
            )}
          </dl>
        );
      })}
    </div>
  );
}

/** `Its cells` section heading line: what the live cells hold, exactly. */
export function ScriptCellsSummary({ family }: { family: ScriptFamilyDetail }) {
  const role =
    family.scriptKind === 'lock'
      ? 'as their lock'
      : family.scriptKind === 'type'
        ? 'as their type'
        : 'as lock or type';
  return (
    <span data-testid="script-cells-summary" className="text-text-dim text-xs">
      {family.liveCellsCount.toLocaleString('en-US')} live cells use {family.name} {role} ·{' '}
      <ExactCkb shannons={BigInt(family.ownedCapacitySum)} signed={false} tone="plain" />,{' '}
      <ExactCkb shannons={BigInt(family.ownedKnowledgeSum)} signed={false} tone="plain" /> of it
      occupied
    </span>
  );
}

export function blockLink(block: number): ReactNode {
  return (
    <Link href={`/blocks/${block}`} className="text-text hover:text-jade">
      {formatBlockNumber(block)}
    </Link>
  );
}
