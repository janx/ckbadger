'use client';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
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
import { HeroFigure, HeroMetric } from '@/components/scripts/script-hero';

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
    <span className="text-text-dim text-body">
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
  inline = false,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  /** A span inside a paragraph (table-rows L1-5) instead of a paragraph of its own. */
  inline?: boolean;
}) {
  const firstDeployed = firstDeployedAt(family);
  const verb =
    family.scriptKind === 'type'
      ? 'decides what'
      : family.scriptKind === 'lock'
        ? 'decides who can spend'
        : 'guards';
  const code = scriptCodeWord(family);
  const last = lineage?.at(-1);
  const Sentence = inline ? 'span' : 'p';
  return (
    <Sentence
      data-testid="script-summary"
      className={inline ? undefined : 'text-body max-w-[640px]'}
    >
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
    </Sentence>
  );
}

/** First deployment of any version, ms (Infinity when none is observed). */
function firstDeployedAt(family: ScriptFamilyDetail): number {
  return Math.min(
    ...family.versions.flatMap((version) => version.deployments.map((d) => d.deployedAt))
  );
}

/**
 * The hero's figure (hierarchy v3, C0′, R13): CKB in its cells as the page's one figure, its live
 * cells (a link down to them) and the occupied share on the same baseline row.
 */
export function ScriptHeroFigure({ family }: { family: ScriptFamilyDetail }) {
  const capacity = BigInt(family.ownedCapacitySum);
  const occupied = BigInt(family.ownedKnowledgeSum);
  if (occupied > capacity)
    throw new Error(`${family.name}: occupied ${occupied} exceeds capacity ${capacity}`);
  const share = capacity === BigInt(0) ? null : scriptShare(occupied, capacity);
  return (
    <HeroFigure
      label="CKB in its cells"
      shannons={capacity}
      companion={
        <>
          in{' '}
          <a href="#its-cells" className="hover:text-text-bright hover:underline">
            {family.liveCellsCount.toLocaleString('en-US')} live cells
          </a>
          {share && ` · ${share} occupied`}
        </>
      }
    />
  );
}

/**
 * The remaining figures in one quiet row (R13; the former facts rail): the code and who holds its
 * code cell, the first deployment and every cell that ever used it.
 */
export function ScriptHeroMetrics({
  family,
  lineage,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
}) {
  const code = scriptCodeWord(family);
  const last = lineage?.at(-1);
  const first = firstDeployedAt(family);
  return (
    <dl
      aria-label="Script figures"
      className="mt-7 grid grid-cols-2 gap-x-8 gap-y-4 lg:grid-cols-4"
    >
      {code && (
        <HeroMetric
          label="Code"
          note={
            code === 'upgradable' && last && lineage!.length > 1
              ? `last replaced ${formatUtc8Date(last.timestamp)}`
              : undefined
          }
        >
          {code === 'immutable'
            ? 'Immutable'
            : `Upgradable · ${formatCount(family.versionsCount, 'version')}`}
        </HeroMetric>
      )}
      {last && (
        <HeroMetric
          label="Code cell held by"
          note={last.custody.witnessState === 'never_signed' ? 'has never signed' : undefined}
        >
          <Link
            href={`/cell/${last.codeCellTxHash}-${last.codeCellOutputIndex}`}
            className="hover:text-jade"
          >
            {holderName(last.custody)}
          </Link>
        </HeroMetric>
      )}
      {Number.isFinite(first) && (
        <HeroMetric label="In use since">{formatUtc8Date(first)}</HeroMetric>
      )}
      <HeroMetric label="Ever used">{formatCount(family.cellsCount, 'cell')}</HeroMetric>
    </dl>
  );
}

function ckbDecimal(shannons: string): string {
  return formatCkbAmount(shannons).full.replaceAll(',', '');
}

/** The history's time axis: where a chart day falls (% of the plot) and the upgrade days on it. */
function historyAxis(
  chart: StackedAreaChartResponse,
  lineage: readonly ScriptHistoryMarkerApi[] | null,
  numbers: ReadonlyMap<string, number>
) {
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
  return { position, upgradeList: [...upgradeDays] };
}

/** Dashed upgrade lines over a panel, on the plot's x (the plots have no axis gutter). */
function UpgradeLines({
  upgradeList,
  position,
  attribute,
}: {
  upgradeList: readonly [string, number[]][];
  position: (day: string) => number | null;
  attribute: 'data-upgrade-line' | 'data-upgrade-tick';
}) {
  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-y-0 left-0 right-[10px]">
      {upgradeList.map(([day]) => {
        const at = position(day);
        return at === null ? null : (
          <span
            key={day}
            {...{ [attribute]: day }}
            className="absolute inset-y-0 border-l border-dashed border-[#4b5266]"
            style={{ left: `${at}%` }}
          />
        );
      })}
    </div>
  );
}

/**
 * The hero's picture (Script board History, hierarchy v3 R13): CKB in its cells with the occupied
 * part in jade (300 px, its name and values inside the plot), then occupied on its own scale; the
 * days its code was replaced dashed through both. One hover state for the whole history (`active`).
 */
export function ScriptHistoryFigure({
  family,
  chart,
  lineage,
  numbers,
  active,
  onActive,
}: {
  family: ScriptFamilyDetail;
  chart: StackedAreaChartResponse;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  numbers: ReadonlyMap<string, number>;
  active: number | null;
  onActive: (index: number | null) => void;
}) {
  for (const key of ['used', 'unused'])
    if (!chart.series.some((series) => series.key === key))
      throw new Error(`${family.name} capacity history has no ${key} series`);
  const data = chart.data.map((point) => ({
    label: point.date,
    values: { used: ckbDecimal(point.values.used), unused: ckbDecimal(point.values.unused) },
  }));
  const { position, upgradeList } = historyAxis(chart, lineage, numbers);
  const shared = {
    data,
    unit: 'CKB',
    insideYTicks: true,
    hideCaption: true,
    activeIndex: active,
    onActiveIndexChange: onActive,
    formatValue: (value: string) => formatExactDecimal(value, 'script cell capacity'),
  } as const;
  return (
    <div data-testid="script-history" className="relative mt-[18px] min-w-0">
      <UpgradeLines upgradeList={upgradeList} position={position} attribute="data-upgrade-line" />
      <Chart
        {...shared}
        title={`CKB in ${family.name} cells`}
        subtitle="Occupied and free capacity, indexed chain day"
        plotName={`CKB in ${family.name} cells · occupied and free`}
        kind="stacked"
        height={300}
        series={[
          { key: 'used', label: 'Occupied', color: DATA_COLOR },
          { key: 'unused', label: 'Free', color: HELD_CAPACITY_COLOR },
        ]}
      />
      <div className="mt-2">
        <Chart
          {...shared}
          title="Occupied by those cells' own bytes"
          subtitle="Occupied capacity on its own scale"
          plotName="occupied by those cells' own bytes"
          kind="area"
          height={96}
          series={[{ key: 'used', label: 'Occupied', color: DATA_COLOR }]}
        />
      </div>
    </div>
  );
}

/**
 * The code track under the history, on the same time axis: the versions deployed on each upgrade
 * day and the band of every code cell holder.
 */
export function ScriptCodeTrack({
  chart,
  lineage,
  numbers,
  holders,
}: {
  chart: StackedAreaChartResponse;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  numbers: ReadonlyMap<string, number>;
  holders: readonly CodeHolder[];
}) {
  if (!lineage || chart.data.length === 0) return null;
  const { position, upgradeList } = historyAxis(chart, lineage, numbers);
  return (
    <div aria-label="Code track" className="text-meta relative mt-1 min-w-0">
      <UpgradeLines upgradeList={upgradeList} position={position} attribute="data-upgrade-tick" />
      <div className="text-text-dim pt-1.5">code · who held the code cell</div>
      <div className="relative h-8 pr-[10px]">
        <div className="relative h-full">
          {upgradeList.map(([day, versions], i) => {
            const at = position(day);
            return at === null ? null : (
              <span
                key={day}
                data-upgrade-day={day}
                data-row={i % 2}
                className="text-text-bright absolute whitespace-nowrap pl-1"
                style={{ left: `${at}%`, top: i % 2 ? 14 : 0 }}
              >
                {versions.map((n) => `v${n}`).join(' · ')}
              </span>
            );
          })}
        </div>
      </div>
      <div className="relative mt-1 pr-[10px]" style={{ height: 16 + holders.length * 14 }}>
        <div className="relative h-full">
          {holders.map((holder, i) => {
            const start = position(holder.from.indexedDay);
            if (start === null) return null;
            const end = holder.until ? (position(holder.until.indexedDay) ?? 100) : 100;
            // Past the middle a label hangs from its band's end, so it never runs off the
            // page; each holder's label has its own row, so labels never overlap.
            const anchor = start > 50 ? { right: `${100 - end}%` } : { left: `${start}%` };
            return (
              <div key={holder.lockArgs}>
                <div
                  data-custodian={holder.lockArgs}
                  className={`absolute top-0 h-2 rounded-sm border ${holder.neverSigned ? 'border-dashed' : ''}`}
                  style={{
                    left: `${start}%`,
                    width: `${Math.max(end - start, 0)}%`,
                    borderColor: 'rgba(160,168,190,.7)',
                    backgroundColor: holder.neverSigned ? 'transparent' : 'rgba(160,168,190,.22)',
                  }}
                />
                <span
                  data-custodian-label={holder.lockArgs}
                  data-row={i}
                  className="absolute whitespace-nowrap text-[#a0a8be]"
                  style={{ ...anchor, top: 12 + i * 14 }}
                >
                  {holderName(holder.custody)}
                  {signingText(holder) ? ` · ${signingText(holder)}` : ''}
                </span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

/**
 * The history's one reading (R13), under the picture: the hovered day — else the last full day —
 * CKB held, occupied and free (exact, in ink; the swatches carry the series colours), and the code
 * version with its holder on that day.
 */
export function ScriptHistoryReading({
  chart,
  lineage,
  numbers,
  active,
}: {
  chart: StackedAreaChartResponse;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  numbers: ReadonlyMap<string, number>;
  active: number | null;
}) {
  const index = active ?? chart.data.length - 1;
  const point = chart.data[index];
  const used = point ? BigInt(point.values.used) : null;
  const unused = point ? BigInt(point.values.unused) : null;
  const deployment = point && lineage ? deploymentOnDay(lineage, point.date) : null;
  const version = deployment?.versionHash != null ? numbers.get(deployment.versionHash) : undefined;
  return (
    <div
      role="status"
      aria-live="polite"
      aria-label="History reading"
      data-testid="script-history-reading"
      className="border-base-border-subtle text-small text-text-dim mt-2.5 flex flex-wrap items-baseline gap-x-6 gap-y-1.5 border-t pt-2"
    >
      {point && used !== null && unused !== null ? (
        <>
          <span className="text-text-bright">
            {point.date}
            {active === null && <span className="text-text-dim"> · last full day</span>}
          </span>
          <span className="flex items-baseline gap-1.5">
            <span>CKB held</span> <ExactCkb shannons={used + unused} signed={false} tone="plain" />
          </span>
          <span className="flex items-baseline gap-1.5">
            <span
              aria-hidden="true"
              className="inline-block h-2 w-2 rounded-sm"
              style={{ backgroundColor: DATA_COLOR }}
            />
            <span>occupied</span> <ExactCkb shannons={used} signed={false} tone="plain" />
          </span>
          <span className="flex items-baseline gap-1.5">
            <span
              aria-hidden="true"
              className="inline-block h-2 w-2 rounded-sm"
              style={{ backgroundColor: HELD_CAPACITY_COLOR }}
            />
            <span>free</span> <ExactCkb shannons={unused} signed={false} tone="plain" />
          </span>
          {deployment && (
            <span className="flex items-baseline gap-1.5">
              <span>code</span>{' '}
              <span className="text-text-bright">{version ? `v${version}` : 'unresolved'}</span>{' '}
              <span>held by {holderName(deployment.custody)}</span>
            </span>
          )}
        </>
      ) : (
        <span>No capacity history in this range.</span>
      )}
    </div>
  );
}

/**
 * How to read the history (tier 2 since hierarchy v3, C2): what occupied and free mean, the dashed
 * upgrade days, and the peak of CKB held with the occupied part now.
 */
export function ScriptHistoryNotes({
  family,
  chart,
  lineage,
  inline = false,
}: {
  family: ScriptFamilyDetail;
  chart: StackedAreaChartResponse | undefined;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  /** Spans inside a paragraph (table-rows L1-5) instead of two paragraphs. */
  inline?: boolean;
}) {
  if (chart && chart.totalPeak === undefined)
    throw new Error(`${family.name} capacity history has no totalPeak`);
  const peak = chart?.totalPeak;
  const capacity = BigInt(family.ownedCapacitySum);
  const occupiedNow = BigInt(family.ownedKnowledgeSum);
  const Sentence = inline ? 'span' : 'p';
  const block = inline ? undefined : 'text-body max-w-[640px]';
  return (
    <>
      <Sentence className={block}>
        A cell must hold enough CKB to pay for its own bytes.{' '}
        <span style={{ color: DATA_COLOR }}>Occupied</span> is that cost for every cell{' '}
        {family.name} guards; <span className="text-[#a0a8be]">free</span> is CKB its owners keep
        there.
        {lineage && lineage.length > 1 ? ' Dashed lines mark the days its code was replaced.' : ''}
      </Sentence>
      {inline && peak && ' '}
      {peak && (
        <Sentence data-testid="script-history-peak" className={block}>
          CKB held peaked at <ExactCkb shannons={BigInt(peak.value)} signed={false} tone="plain" />{' '}
          on {peak.date}
          {capacity > BigInt(0) && (
            <>
              {' '}
              · occupied is now <ExactCkb
                shannons={occupiedNow}
                signed={false}
                tone="plain"
              />, {scriptShare(occupiedNow, capacity)} of what the cells hold
            </>
          )}
          {inline && '.'}
        </Sentence>
      )}
    </>
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
        <p data-role="caption" className="text-body max-w-[640px]">
          No one. {family.name} cells name their code by its data hash, or through a code cell that
          can never be spent, so the code they run cannot be replaced.
        </p>
      );
    return (
      <div className="text-small space-y-3">
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
    <>
      <ol
        aria-label="Code cell holders"
        className="grid grid-cols-1 gap-x-6 gap-y-6 md:[grid-template-columns:repeat(var(--holders),minmax(0,1fr))]"
        style={{ ['--holders' as string]: holders.length }}
      >
        {holders.map((holder, i) => (
          <li key={holder.lockArgs} data-holder={holder.lockArgs} className="text-small min-w-0">
            <div className="text-text-dim text-meta">
              {holder.until
                ? `${formatUtc8Date(holder.from.timestamp)} → ${formatUtc8Date(holder.until.timestamp)}`
                : `since ${formatUtc8Date(holder.from.timestamp)}`}
            </div>
            <div className="text-text-bright text-body mt-1">
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
            <div className="text-text-dim text-meta mt-0.5">
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
                    className="text-meta flex items-center gap-2"
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
              <div className="text-meta mt-3 rounded border border-dashed border-[#343c50] p-3 text-[#a0a8be]">
                Its keys and threshold appear on chain the first time it signs.
              </div>
            ) : null}
          </li>
        ))}
      </ol>
      {handovers.map((handover) => (
        <p key={handover.day} className="text-text-dim text-meta mt-2.5">
          ● kept across the handover on {handover.day}: {handover.common} of {handover.total} keys ·
          ○ replaced: {handover.total - handover.common} of {handover.total}
        </p>
      ))}
      <p data-role="caption" className="text-text text-body mt-3.5 max-w-[640px]">
        Whoever can spend its code cell can put new code in it, and every {family.name} cell runs it
        from the next block. {NUMBER_WORDS[holders.length] ?? holders.length}{' '}
        {allMultisig
          ? holders.length === 1
            ? 'multisig has'
            : 'multisigs have'
          : holders.length === 1
            ? 'holder has'
            : 'holders have'}{' '}
        held it.
      </p>
    </>
  );
}

/**
 * How a type-id script's code can change (tier 2 since hierarchy v3, C2): the mechanism behind
 * "Who can change its code". Nothing for code named by its data hash.
 */
export function ScriptCustodyMechanism({
  family,
  lineage,
  binaries,
  inline = false,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
  /** The Code versions table's former heading note ("one type id, …, N binaries"). */
  binaries: string;
  /** Spans inside a paragraph (table-rows L1-5) instead of two paragraphs. */
  inline?: boolean;
}) {
  const Sentence = inline ? 'span' : 'p';
  return (
    <>
      {lineage && (
        <Sentence className={inline ? undefined : 'text-body max-w-[640px]'}>
          {family.name} cells point at their code through one code cell&apos;s type id, not at a
          fixed binary. Whoever can spend that cell can put new code in it, and every {family.name}{' '}
          cell runs the new code from the next block.
        </Sentence>
      )}
      {inline && lineage && ' '}
      <Sentence className={inline ? undefined : 'text-text-dim text-body max-w-[640px]'}>
        Code versions: {binaries}.
      </Sentence>
    </>
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
    <span data-testid="script-cells-summary" className="text-text-dim text-small">
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
