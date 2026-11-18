'use client';
import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';
import Link from '@/components/ui/link';
import { NoBreak } from '@/components/ui/no-break';
import { ShortId } from '@/components/ui/short-id';
import { Utc8Date } from '@/components/ui/utc8-date';
import { useElementWidth } from '@/hooks/useElementWidth';
import { Chart, alignedPlotLeft, type ChartValueAxisInput } from '@/components/ui/chart';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { DetailFacts } from '@/components/ui/detail-layout';
import {
  ScriptCustodyEvidence,
  immutableHow,
  isImmutableClass,
} from '@/components/scripts/script-code-evidence';
import { RowTag } from '@/components/ui/row-tag';
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
import { BOARD_INKS, DATA_COLOR, HELD_CAPACITY_COLOR, inkStyle } from '@/lib/design-tokens';
import {
  deploymentOnDay,
  holderName,
  holderParts,
  signingText,
  type CodeHolder,
} from '@/lib/script-lineage';
import type { ShortSize } from '@/lib/shorten';
import { HeroFigure, HeroMetric } from '@/components/scripts/script-hero';
import { percentOf } from '@/lib/format-unit';
import { HELD_PLOT_NAME } from '@/components/asset/asset-history';
import { unitNode, unitText } from '@/components/ui/units';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';
import { charPx } from '@/lib/type-metrics';

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

/**
 * The hero line beside the name: "lock script · upgradable" in dim words; immutable code — the
 * rarer, stronger fact — is the line's accent, a jade "immutable" row tag with how it is pinned in
 * its title (lane F follow-up). The name stays the hero's one bright thing.
 */
export function ScriptHeading({ family }: { family: ScriptFamilyDetail }) {
  const code = scriptCodeWord(family);
  return (
    <span className="text-text-dim text-body">
      {roleNoun(family.scriptKind)}
      {code === 'immutable' ? (
        <>
          {' · '}
          <RowTag
            variant="accent"
            title={[
              'immutable',
              ...immutableHow((family.codeClasses ?? []).map((row) => row.class)),
            ].join(' · ')}
          >
            immutable
          </RowTag>
        </>
      ) : code ? (
        ` · ${code}`
      ) : (
        ''
      )}
    </span>
  );
}

/** The holder as the hero's sentence names it, the page's lede: prose, the row size (R9). */
function holderSentence(custody: ScriptCustodyApi): string {
  if (custody.class === 'multisig')
    return custody.witnessState === 'never_signed'
      ? 'a multisig that has never signed'
      : `${holderName(custody, 'row')}`;
  if (custody.class === 'one_key') return 'a single key';
  if (custody.class === 'unspendable') return 'a lock that can never be unlocked';
  return holderName(custody, 'row');
}

/**
 * A sentence of the script's notes: a paragraph of its own at the body step on the measure, or a
 * span inside a caller's paragraph (table-rows L1-5), the tag and its class chosen together (the
 * measure gate reads a paragraph by its tag; review of the rules branch, tests SHOULD-1).
 */
function NoteSentence({
  inline,
  className,
  testId,
  children,
}: {
  inline: boolean;
  className?: string;
  testId?: string;
  children: ReactNode;
}) {
  return inline ? (
    <span data-testid={testId}>{children}</span>
  ) : (
    <p data-testid={testId} className={cn('text-body max-w-measure', className)}>
      {children}
    </p>
  );
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
  return (
    <NoteSentence inline={inline} testId="script-summary">
      {family.name} {verb}{' '}
      <span className="text-text-bright">{family.liveCellsCount.toLocaleString('en-US')}</span> live
      cells holding{' '}
      <span className="text-text-bright">
        <CkbAmount
          form="column"
          shannons={BigInt(family.ownedCapacitySum)}
          signed={false}
          tone="plain"
        />
      </span>
      ; <span className="text-text-bright">{family.cellsCount.toLocaleString('en-US')}</span> cells
      have used it
      {unitNode(Number.isFinite(firstDeployed) ? ` since ${formatUtc8Date(firstDeployed)}` : '')}.
      {code === 'immutable' && ' Its code is immutable: no one can replace it.'}
      {code === 'upgradable' && lineage && last && lineage.length > 1 && (
        <>
          {' '}
          Its code is upgradable: it has been replaced{' '}
          <span className="text-text-bright">
            {unitNode(formatCount(lineage.length - 1, 'time'))}
          </span>
          , last on <Utc8Date timestamp={last.timestamp} />, and its code cell now sits under{' '}
          {holderSentence(last.custody)}.
        </>
      )}
      {code === 'upgradable' && lineage && lineage.length === 1 && (
        <> Its code is upgradable by {holderSentence(lineage[0].custody)}.</>
      )}
    </NoteSentence>
  );
}

type FamilyDeployment = ScriptFamilyDetail['versions'][number]['deployments'][number];

/** The first deployment of any version (lane L: its transaction is the script's deployer tx). */
function firstDeployment(family: ScriptFamilyDetail): FamilyDeployment | null {
  let first: FamilyDeployment | null = null;
  for (const version of family.versions)
    for (const d of version.deployments) if (!first || d.deployedAt < first.deployedAt) first = d;
  return first;
}

/** First deployment of any version, ms (Infinity when none is observed). */
function firstDeployedAt(family: ScriptFamilyDetail): number {
  return firstDeployment(family)?.deployedAt ?? Infinity;
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
  const share = capacity === BigInt(0) ? null : percentOf(occupied, capacity);
  return (
    <HeroFigure
      label="CKB in its cells"
      shannons={capacity}
      companion={[
        <>
          in{' '}
          <a href="#its-cells" className="hover:text-text-bright hover:underline">
            {unitNode(formatCount(family.liveCellsCount, 'live cell'))}
          </a>
        </>,
        share && `${share} occupied`,
      ]}
    />
  );
}

/**
 * The remaining figures in one quiet row (R13; the former facts rail): the code and who holds its
 * code cell, the first deployment and every cell that ever used it.
 */
/**
 * A code cell holder's name (A4 Chrome F2): its id is one unit, a line may break between the kind
 * and the id; a phrase ("an unspendable lock") is words a line may break between. In the hero
 * tile (17 px, a column of (358 − 32) / 2 = 163 px at 390, 148 at 360), below sm the tight size:
 * "multisig 0x6598…d696", 204 px whole, is "multisig" over the 112.2 px id. From sm the row size
 * (R9): "multisig 0x65980a1b…31d696", 265.2 px whole, stands on one line in the 288 px tile at
 * 640 and 284 at 1280, and is "multisig" over the 173.4 px id in the 220 px tile at 1024.
 */
function HolderName({ custody, size }: { custody: ScriptCustodyApi; size: ShortSize }) {
  const { label, id } = holderParts(custody, size);
  return id === null ? (
    <>{label}</>
  ) : (
    <>
      {label} <NoBreak>{id}</NoBreak>
    </>
  );
}

export function ScriptHeroMetrics({
  family,
  lineage,
}: {
  family: ScriptFamilyDetail;
  lineage: readonly ScriptHistoryMarkerApi[] | null;
}) {
  const code = scriptCodeWord(family);
  const last = lineage?.at(-1);
  const deployment = firstDeployment(family);
  // A tile is under 200 px only on a phone (163 px at 390); from sm it is 220 px at its narrowest
  // (1024), where a holder's id at the row size, 17 characters at 17 px (173.4 px), stands on its
  // own line, the size the lede and the tables write it in (R9).
  const size = useTightBelowSm();
  return (
    <dl
      aria-label="Script figures"
      className="mt-7 grid grid-cols-2 gap-x-8 gap-y-4 lg:grid-cols-4"
    >
      {code && (
        <HeroMetric
          label="Code"
          note={
            code === 'upgradable' && last && lineage!.length > 1 ? (
              <>
                last replaced <Utc8Date timestamp={last.timestamp} />
              </>
            ) : undefined
          }
          parts={
            code === 'immutable'
              ? ['Immutable']
              : ['Upgradable', formatCount(family.versionsCount, 'version')]
          }
        />
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
            {/* a hero tile: tight below sm, the row size from it (R9) */}
            <HolderName custody={last.custody} size={size} />
          </Link>
        </HeroMetric>
      )}
      {deployment && (
        <HeroMetric
          label="In use since"
          note={
            <>
              deployed in{' '}
              <Link
                href={`/tx/${deployment.codeCellTxHash}`}
                title={deployment.codeCellTxHash}
                className="hover:text-jade"
              >
                <ShortId value={deployment.codeCellTxHash} kind="hash" size={size} />
              </Link>
            </>
          }
        >
          <Utc8Date timestamp={deployment.deployedAt} />
        </HeroMetric>
      )}
      <HeroMetric label="Ever used">{unitNode(formatCount(family.cellsCount, 'cell'))}</HeroMetric>
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

/** Dashed upgrade lines over a panel, on the plot's x: from the plots' value gutter (`left`). */
function UpgradeLines({
  upgradeList,
  position,
  attribute,
  left,
}: {
  upgradeList: readonly [string, number[]][];
  position: (day: string) => number | null;
  attribute: 'data-upgrade-line' | 'data-upgrade-tick';
  left: number;
}) {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-y-0 right-[10px]"
      style={{ left: `${left}px` }}
    >
      {upgradeList.map(([day]) => {
        const at = position(day);
        return at === null ? null : (
          <span
            key={day}
            {...{ [attribute]: day }}
            className="absolute inset-y-0 border-l border-dashed"
            style={{ left: `${at}%`, borderColor: BOARD_INKS.quiet }}
          />
        );
      })}
    </div>
  );
}

/** A version label's advance at the meta step (`text-meta`). */
export const LABEL_ADVANCE = charPx('meta');
/** px between a version label and its dashed line, and between two labels or a label and the name. */
const LABEL_PAD = 4;
const LABEL_GAP = 8;

/**
 * The version days written inside the main plot (board ScriptDetail: "v3 · v4", "v5" in ghost ink on
 * the name's line), one label per deployment day. The newest day is placed first; a label that
 * would touch the plot's name or a label already placed is left out — its dashed line stays, and
 * its versions are the timeline's ticks and the versions table's rows.
 */
function UpgradeLabels({
  upgradeList,
  position,
  width,
  plotName,
  left,
}: {
  upgradeList: readonly [string, number[]][];
  position: (day: string) => number | null;
  width: number | null;
  plotName: string;
  /** The plots' value gutter: the plot, and these labels, start after it. */
  left: number;
}) {
  if (width === null) return null;
  const plot = width - 10 - left;
  const taken: [number, number][] = [[0, LABEL_PAD * 2 + plotName.length * LABEL_ADVANCE]];
  const placed: { day: string; text: string; at: number; right: boolean }[] = [];
  const days = upgradeList
    .map(([day, versions]) => ({ day, versions, at: position(day) }))
    .filter((d): d is { day: string; versions: number[]; at: number } => d.at !== null)
    .sort((a, b) => b.at - a.at);
  for (const { day, versions, at } of days) {
    const text = versions.map((n) => `v${n}`).join(' · ');
    const w = text.length * LABEL_ADVANCE;
    const x = (at / 100) * plot;
    const right = x + LABEL_PAD + w > plot;
    const box: [number, number] = right ? [x - LABEL_PAD - w, x] : [x, x + LABEL_PAD + w];
    if (taken.some(([a, b]) => box[0] < b + LABEL_GAP && a < box[1] + LABEL_GAP)) continue;
    taken.push(box);
    placed.push({ day, text, at, right });
  }
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute right-[10px] top-0 h-[28px]"
      style={{ left: `${left}px` }}
    >
      {placed
        .sort((a, b) => a.at - b.at)
        .map(({ day, text, at, right }) => (
          <span
            key={day}
            data-upgrade-label={day}
            className={`text-meta text-text-dim leading-label absolute top-[5px] whitespace-nowrap ${right ? 'pr-1' : 'pl-1'}`}
            style={right ? { right: `${100 - at}%` } : { left: `${at}%` }}
          >
            {text}
          </span>
        ))}
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
  const [ref, width] = useElementWidth<HTMLDivElement>();
  for (const key of ['used', 'unused'])
    if (!chart.series.some((series) => series.key === key))
      throw new Error(`${family.name} capacity history has no ${key} series`);
  const data = chart.data.map((point) => ({
    label: point.date,
    values: { used: ckbDecimal(point.values.used), unused: ckbDecimal(point.values.unused) },
  }));
  const { position, upgradeList } = historyAxis(chart, lineage, numbers);
  // The token and collection plots' name: the same stack (occupied, free above it) in the same
  // unit; the page's h1 says which script, and a script's name has no length bound (lane A5).
  const plotName = HELD_PLOT_NAME;
  const held: ChartValueAxisInput = {
    title: `CKB in ${family.name} cells`,
    data,
    unit: 'CKB',
    kind: 'stacked',
    bareTicks: true,
    series: [
      { key: 'used', label: 'Occupied', color: DATA_COLOR },
      { key: 'unused', label: 'Free', color: HELD_CAPACITY_COLOR },
    ],
  };
  const occupied: ChartValueAxisInput = {
    title: "Occupied by those cells' own bytes",
    data,
    unit: 'CKB',
    kind: 'area',
    bareTicks: true,
    series: [{ key: 'used', label: 'Occupied', color: DATA_COLOR }],
  };
  // U-19: the round values sit in one gutter left of both plots, in the dim ink (inside, a spike
  // crossed them), bare, each plot's name saying CKB once; the upgrade lines and labels start
  // after the gutter, on the plots' x.
  const plotLeft = alignedPlotLeft([held, occupied]);
  const shared = {
    plotLeft,
    hideCaption: true,
    activeIndex: active,
    onActiveIndexChange: onActive,
    formatValue: (value: string) => formatExactDecimal(value, 'script cell capacity'),
  } as const;
  return (
    <div ref={ref} data-testid="script-history" className="relative mt-[18px] min-w-0">
      <UpgradeLines
        upgradeList={upgradeList}
        position={position}
        attribute="data-upgrade-line"
        left={plotLeft}
      />
      <UpgradeLabels
        upgradeList={upgradeList}
        position={position}
        width={width}
        plotName={plotName}
        left={plotLeft}
      />
      <Chart
        {...shared}
        {...held}
        subtitle="Occupied and free capacity, indexed chain day"
        plotName={plotName}
        height={300}
      />
      <div className="mt-2">
        <Chart
          {...shared}
          {...occupied}
          subtitle="Occupied capacity on its own scale"
          plotName="CKB occupied by those cells' own bytes"
          height={96}
        />
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
  const swatch = (color: string) => (
    <span
      aria-hidden="true"
      className="mr-1.5 inline-block h-2 w-2"
      style={{ backgroundColor: color }}
    />
  );
  return (
    <div
      role="status"
      aria-live="polite"
      aria-label="History reading"
      data-testid="script-history-reading"
      className="border-row-rule text-small mt-2.5 border-t pt-2"
    >
      {point && used !== null && unused !== null ? (
        // The reading as labelled facts, the detail pages' dl rows (readability R4, lane B7): five
        // facts in a row of their own words. A series' swatch leads its value.
        <DetailFacts
          facts={[
            {
              label: 'day',
              value: (
                <>
                  {unitNode(point.date)}
                  {active === null && <span className="text-text-dim"> · last full day</span>}
                </>
              ),
            },
            {
              label: 'CKB held',
              value: (
                <CkbAmount form="column" shannons={used + unused} signed={false} tone="plain" />
              ),
            },
            {
              label: 'occupied',
              value: (
                <>
                  {swatch(DATA_COLOR)}
                  <CkbAmount form="column" shannons={used} signed={false} tone="plain" />
                </>
              ),
            },
            {
              label: 'free',
              value: (
                <>
                  {swatch(HELD_CAPACITY_COLOR)}
                  <CkbAmount form="column" shannons={unused} signed={false} tone="plain" />
                </>
              ),
            },
            ...(deployment
              ? [
                  {
                    label: 'code',
                    value: (
                      <>
                        {version ? `v${version}` : 'unresolved'}{' '}
                        {/* tight: the readout is tier 1's (it kept a 160-character budget as one
                            line, __tests__/utils/hierarchy.ts) */}
                        <span className="text-text-dim">held by</span>{' '}
                        <span className="text-text">{holderName(deployment.custody, 'tight')}</span>
                      </>
                    ),
                  },
                ]
              : []),
          ]}
        />
      ) : (
        <span className="text-text-dim">No capacity history in this range.</span>
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
  return (
    <>
      <NoteSentence inline={inline}>
        A cell must hold enough CKB to pay for its own bytes.{' '}
        <span style={{ color: DATA_COLOR }}>Occupied</span> is that cost for every cell{' '}
        {family.name} guards; <span className="text-text">free</span> is CKB its owners keep there.
        {lineage && lineage.length > 1 ? ' Dashed lines mark the days its code was replaced.' : ''}
      </NoteSentence>
      {inline && peak && ' '}
      {peak && (
        <NoteSentence inline={inline} testId="script-history-peak">
          CKB held peaked at{' '}
          <CkbAmount form="column" shannons={BigInt(peak.value)} signed={false} tone="plain" /> on{' '}
          {unitNode(peak.date)}
          {capacity > BigInt(0) && (
            <>
              {' '}
              · occupied is now{' '}
              <CkbAmount form="column" shannons={occupiedNow} signed={false} tone="plain" />,{' '}
              {percentOf(occupiedNow, capacity)} of what the cells hold
            </>
          )}
          {inline && '.'}
        </NoteSentence>
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
        <p data-role="caption" className="text-body max-w-measure">
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
              {unitNode(
                holder.until
                  ? `${formatUtc8Date(holder.from.timestamp)} → ${formatUtc8Date(holder.until.timestamp)}`
                  : `since ${formatUtc8Date(holder.from.timestamp)}`
              )}
            </div>
            <div className="text-text-bright text-body mt-1">
              {i > 0 && (
                <span
                  aria-hidden="true"
                  className="text-[color:var(--ink)]"
                  style={inkStyle(BOARD_INKS.quiet)}
                >
                  →{' '}
                </span>
              )}
              {holderName(holder.custody, 'row')}
            </div>
            {signingText(holder) && (
              <div className={holder.neverSigned ? 'text-text-dim mt-1' : 'text-text mt-1'}>
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
              <ul
                aria-label={`Keys of ${holderName(holder.custody, 'row')}`}
                className="mt-3 space-y-1.5"
              >
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
                        fill={kept(i, key) ? 'var(--c-text)' : 'var(--c-base-bg)'}
                        stroke="var(--c-text)"
                      />
                    </svg>
                    <ShortId
                      value={key}
                      kind="hash"
                      size="row"
                      className={kept(i, key) ? 'text-text-bright' : 'text-text-dim'}
                    />
                  </li>
                ))}
              </ul>
            ) : holder.neverSigned ? (
              <div className="text-meta border-text-ghost text-text mt-3 border border-dashed p-3">
                Its keys and threshold appear on chain the first time it signs.
              </div>
            ) : null}
          </li>
        ))}
      </ol>
      {handovers.map((handover) => (
        <p key={handover.day} className="text-text-dim text-meta max-w-measure mt-2.5">
          ● kept across the handover on {unitNode(handover.day)}: {handover.common} of{' '}
          {unitNode(formatCount(handover.total, 'key'))} · ○ replaced:{' '}
          {handover.total - handover.common} of {handover.total}
        </p>
      ))}
      <p data-role="caption" className="text-text text-body max-w-measure mt-3.5">
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
  return (
    <>
      {lineage && (
        <NoteSentence inline={inline}>
          {family.name} cells point at their code through one code cell&apos;s type id, not at a
          fixed binary. Whoever can spend that cell can put new code in it, and every {family.name}{' '}
          cell runs the new code from the next block.
        </NoteSentence>
      )}
      {inline && lineage && ' '}
      <NoteSentence inline={inline} className="text-text-dim">
        Code versions: {unitText(binaries)}.
      </NoteSentence>
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
      <CkbAmount
        form="column"
        shannons={BigInt(family.ownedCapacitySum)}
        signed={false}
        tone="plain"
      />
      ,{' '}
      <CkbAmount
        form="column"
        shannons={BigInt(family.ownedKnowledgeSum)}
        signed={false}
        tone="plain"
      />{' '}
      of it occupied
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
