'use client';
import { useState, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { CellBar } from '@/components/ui/cell-bar';
import { RangeSelect } from '@/components/ui/range-select';
import { SectionDisclosure } from '@/components/ui/section-disclosure';
import { Pager } from '@/components/ui/pager';
import { rankedPageAt, type Asked, type CursorPageRead, type CursorWalk } from '@/lib/pagination';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
import { CELL_GAP_PX, ListTable, type ListColumn } from '@/components/ui/list-table';
import { Dotted, Labelled, PhoneRow } from '@/components/ui/phone-row';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { CkbAmount } from '@/components/ui/ckb-amount';
import {
  ScriptCustodyEvidence,
  DEPRECATED_TITLE,
  isImmutableClass,
  scriptClassLabel,
} from '@/components/scripts/script-code-evidence';
import { ScriptByteMap } from '@/components/ui/script-byte-map';
import { HolderTimeline } from '@/components/scripts/holder-timeline';
import { ScriptNewestCells, type NewestCellsState } from '@/components/scripts/script-newest-cells';
import {
  ScriptCellsSummary,
  ScriptCustodyFigure,
  ScriptCustodyMechanism,
  ScriptHeading,
  ScriptHeroFigure,
  ScriptHeroMetrics,
  ScriptHistoryFigure,
  ScriptHistoryNotes,
  ScriptHistoryReading,
  ScriptSummarySentence,
  blockLink,
} from '@/components/scripts/script-family-board';
import type { Cell, ScriptFamilyDetail, StackedAreaChartResponse } from '@/lib/api';
import { CAPACITY_RANGE_OPTIONS, type CapacityRangeKey } from '@/lib/capacity-range';
import { formatCapacity, formatCkbAmount, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { PHONE_PART_GAP_PX, columnPx, fontPx, phoneHead, textPx } from '@/lib/type-metrics';
import { MIN_TRACK_PX } from '@/components/ui/aligned-bar';
import {
  UNAVAILABLE,
  WIDEST_BLOCK_NUMBER,
  WIDEST_CELL_CAPACITY,
  WIDEST_CELL_CAPACITY_SUMMARY,
  WIDEST_CELL_OCCUPIED,
  WIDEST_CODE_CELL_INDEX,
  WIDEST_COUNT_DIGITS,
  WIDEST_ROW_COUNT,
  WIDEST_SCRIPT_VERSION,
  shortChars,
} from '@/lib/list-bounds';
import { SentenceNodes, sentenceText, type SentencePart } from '@/components/ui/block-link';
import { formatCount } from '@/lib/format-count';
import { cn } from '@/lib/utils';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { RowTag } from '@/components/ui/row-tag';
import {
  codeHolders,
  holderName,
  orderedVersions,
  signerOf,
  typeLineage,
  versionNumbers,
} from '@/lib/script-lineage';
import { useDocumentTitle } from '@/lib/use-document-title';
import { ShortId } from '@/components/ui/short-id';
import { SHORT_FORMS, shortenHash, type ShortSize } from '@/lib/shorten';
import { Utc8Date } from '@/components/ui/utc8-date';
import { unitNode } from '@/components/ui/units';

/** One code cell of a version, as its own `/cells` request resolved (or failed). */
export interface CodeCellState {
  cell?: Cell;
  error?: Error | null;
}

export interface ScriptFamilyPageProps {
  family: ScriptFamilyDetail;
  deprecated: boolean;
  chart: StackedAreaChartResponse | undefined;
  chartLoading: boolean;
  chartError: Error | null;
  capacityRange: CapacityRangeKey;
  onCapacityRange: (range: CapacityRangeKey) => void;
  /** `versionHash → live cells using it` cell, as the usage endpoint answered. */
  versionUsage: (versionHash: string) => ReactNode;
  codeCells: ReadonlyMap<string, CodeCellState>;
  /** Lane S: the newest live cells, from `/cells/by-script?order=desc` (one index). */
  newest: NewestCellsState;
  /**
   * The family's cells, a page of them, merged from its references' pages, as the walk asked for
   * it: the pager's figures and ways are lib/pagination.ts rankedPageAt's, from the page alone.
   */
  cells: {
    /** The merged page as its walk asked for it (lane cd-fixes-controls, item 1); none yet. */
    read: Asked<CursorPageRead & { data: readonly Cell[] }> | null;
    walk: CursorWalk;
    loading: boolean;
  };
}

function CellOwner({ cell, size }: { cell: Cell; size: ShortSize }) {
  return cell.address ? (
    <Link href={`/address/${cell.address}`} look="inline">
      <ShortId value={cell.address} kind="address" size={size} />
    </Link>
  ) : (
    <span role="alert" className="text-negative">
      Owner address unavailable
    </span>
  );
}

/**
 * The type script beside the lock: its registry name (R6), or its code hash when unregistered, at
 * the form's size (U-09).
 */
function AlsoGuardedBy({ cell, size }: { cell: Cell; size: ShortSize }) {
  if (!cell.typeScriptHash) return <span className="text-text-dim">—</span>;
  if (!cell.typeCodeHash || !cell.typeHashType)
    return (
      <span role="alert" className="text-negative">
        Type reference unavailable
      </span>
    );
  if (cell.typeScriptName === undefined)
    throw new Error(`Cell ${cell.txHash}:${cell.outputIndex} row has no typeScriptName (R6)`);
  return (
    <Link href={`/script/${cell.typeCodeHash}?hashType=${cell.typeHashType}`} look="inline">
      {cell.typeScriptName ?? <ShortId value={cell.typeCodeHash} kind="hash" size={size} />}
    </Link>
  );
}

/** Occupied, in bytes (1 CKB of capacity pays for 1 byte): the board writes `63 B`, not CKB. */
function OccupiedBytes({ cell }: { cell: Cell }) {
  const toBytes = (shannons: string) => {
    const value = BigInt(shannons);
    if (value % BigInt(100000000) !== BigInt(0))
      throw new Error(
        `Cell ${cell.txHash}:${cell.outputIndex} occupies ${value} shannons, not whole bytes`
      );
    return `${(value / BigInt(100000000)).toLocaleString('en-US')} B`;
  };
  if (cell.cellType === 'genesis_special_burn') {
    return cell.virtualCommonKnowledgeSize === undefined ? (
      <span
        role="alert"
        data-testid="burn-cell-virtual-size-error"
        title={`Genesis burn cell ${cell.txHash}:${cell.outputIndex} has no virtualCommonKnowledgeSize`}
        className="text-negative"
      >
        unavailable
      </span>
    ) : (
      <span
        className="text-text"
        title={`Virtual common knowledge size: ${formatCapacity(cell.virtualCommonKnowledgeSize)}`}
      >
        {toBytes(cell.virtualCommonKnowledgeSize)}
      </span>
    );
  }
  return <span className="text-text">{toBytes(cell.commonKnowledgeSize)}</span>;
}

function CellAnatomy({ cell, className }: { cell: Cell; className?: string }) {
  const bytes = cell.commonKnowledgeSizeBreakdown;
  if (!bytes)
    return (
      <span role="alert" className="text-negative">
        Cell anatomy unavailable
      </span>
    );
  return (
    <CellBar
      subject={`${cell.txHash}:${cell.outputIndex}`}
      capacity={cell.capacity}
      className={className}
      hideLegend
      anatomy={{
        lock: String(bytes.lockScriptBytes),
        type: String(bytes.typeScriptBytes),
        data: String(bytes.dataBytes),
      }}
    />
  );
}

const ANATOMY_LEGEND = [
  ['capacity', 'var(--c-text-bright)'],
  ['lock', 'var(--c-cell-lock)'],
  ['type', 'var(--c-cell-type)'],
  ['data', 'var(--c-cell-data)'],
  ['free', 'var(--c-bar-track)'],
] as const;

type ReferenceRow = NonNullable<ScriptFamilyDetail['references']>[number];

/** A reference's live code cells: the deployments of its current version that are live. */
function liveDeployments(family: ScriptFamilyDetail, reference: ReferenceRow) {
  const version = family.versions.find((v) => v.versionHash === reference.currentVersionHash);
  return { version, live: version?.deployments.filter((d) => d.status === 'live') ?? [] };
}

/** One registered reference in full: the former tier-2 definition list (table-rows L1-5). */
function ReferenceFacts({
  family,
  reference,
}: {
  family: ScriptFamilyDetail;
  reference: ReferenceRow;
}) {
  const { version, live } = liveDeployments(family, reference);
  const term = 'text-text-dim';
  const value = 'm-0 min-w-0 [overflow-wrap:anywhere]';
  return (
    <dl className="border-row-rule grid grid-cols-[110px_minmax(0,1fr)] gap-x-3 gap-y-1 border-t pt-3 first:border-t-0 first:pt-0">
      <dt className={term}>code hash</dt>
      <dd className={value}>{reference.referenceHash}</dd>
      <dt className={term}>hash type</dt>
      <dd className={value}>
        {reference.hashType} ·{' '}
        {reference.hashType === 'type' ? 'follows every upgrade' : 'pins this exact binary'} ·{' '}
        {unitNode(formatCount(reference.liveCellsCount, 'live cell'))}
      </dd>
      <dt className={term}>current code</dt>
      <dd className={value}>{reference.currentVersionHash ?? 'Retired · no current code cell'}</dd>
      {reference.hashType === 'type' && version && (
        <>
          <dt className={term}>pin instead</dt>
          <dd className={cn(value, 'text-text-dim', 'max-w-measure')}>
            its data hash with hash type data1 or data2 pins this exact binary; an upgrade will not
            reach cells that pin it
          </dd>
        </>
      )}
      <dt className={term}>cell dep</dt>
      <dd className={value}>
        {live.length ? (
          live.map((deployment) => (
            <span
              key={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
              className="block"
            >
              <Link
                href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                look="inline"
              >
                {deployment.codeCellTxHash}
                <OutpointIndex index={deployment.codeCellOutputIndex} />
              </Link>
              <span className="text-text-dim">
                {' '}
                · dep type code · the live code cell of the current version
              </span>
            </span>
          ))
        ) : (
          <span className="text-text-dim">
            {reference.currentVersionHash
              ? 'No live code cell for the current version'
              : 'Retired: no current code cell'}
          </span>
        )}
      </dd>
      <dt className={term}>held by</dt>
      <dd className={value}>
        {reference.custody ? (
          <ScriptCustodyEvidence custody={reference.custody} />
        ) : (
          <span className="text-text-dim">Evidence unavailable</span>
        )}
      </dd>
    </dl>
  );
}

/**
 * What a column of Registered references needs for its head (readability 2026-11-18: "hash type"
 * overran its column, read "hash typecode hash"): the head on one line at the meta step, 0.6 em a
 * character in the mono face, and the cell's 16 px right padding (`pr-4`) before the next column.
 */
export function headBudget(label: string): number {
  return Math.ceil(textPx(label.length, fontPx('meta'))) + 16;
}

/**
 * What the family's tables hold (readability F3-e): each column's widest value, measured at its
 * step through columnPx, with the 16 px gap after every column but the last.
 */
const small = (text: string) => textPx(text.length, fontPx('small'));
const meta = (text: string) => textPx(text.length, fontPx('meta'));
/** A hash at the row size, as a run of its characters ("0x12345678…abcdef"). */
const ROW_HASH = 'x'.repeat(shortChars(SHORT_FORMS.row.hash));
/** A code cell's out point: a deployment transaction writes a few cells (WIDEST_CODE_CELL_INDEX). */
const CODE_CELL_OUTPOINT = `${ROW_HASH}:${WIDEST_CODE_CELL_INDEX}`;
/** A live cell's out point: a transaction's outputs are under 10,000 (four digits). */
const CELL_OUTPOINT = `${ROW_HASH}:${WIDEST_COUNT_DIGITS}`;
/** A version's number: two digits (WIDEST_SCRIPT_VERSION). */
const VERSION_NUMBER = WIDEST_SCRIPT_VERSION;
// a cell's capacity and occupied bytes: lib/list-bounds.ts's WIDEST_CELL_CAPACITY and
// WIDEST_CELL_OCCUPIED (lane P2's follow-up, item 7: they were kept here)
const WIDEST_OCCUPIED = WIDEST_CELL_OCCUPIED;

/**
 * Line 1 of a script's cell on a phone (the phone-rows board's head()): the out point at its tight
 * size with an index of four digits at the key's 14 px ("0x1234…abcd:9999", 16 characters, 134.4;
 * no stone, as the table's), the 12 px between the parts and the cell's capacity in the summary
 * form at its widest, "9,999,999,999" (WIDEST_CELL_CAPACITY cut to whole CKB, 13 characters at
 * 13 px, 101.4) and " CKB" at the unit's 12 px (28.8): 276.6 of a 360 px phone's 328 px line.
 */
const SCRIPT_CELL_PHONE_HEAD_PX = phoneHead(
  'script cells',
  textPx(shortChars(SHORT_FORMS.tight.hash) + `:${WIDEST_COUNT_DIGITS}`.length, fontPx('body')) +
    PHONE_PART_GAP_PX +
    textPx(WIDEST_CELL_CAPACITY_SUMMARY.length, fontPx('small')) +
    textPx(' CKB'.length, fontPx('meta'))
);

/**
 * Tier 2's "The script" (consistency C6, board ScriptDetail): its cells in one line, the script's
 * shape drawn once (code_hash 32 · hash_type 1, then the args each cell sets), and one row per
 * registered reference — hash type, code hash, live cells, the cell dep that serves it, who holds
 * its code cell. Each reference's full facts (the former definition lists) are folded beneath.
 */
function ScriptFacts({ family }: { family: ScriptFamilyDetail }) {
  const references: ReferenceRow[] = family.references ?? [];
  const kind =
    family.scriptKind === 'lock' || family.scriptKind === 'type' ? family.scriptKind : undefined;
  // a reference's live code cell, at the form's size (U-09): row in the table, tight in the row
  const depOf = (reference: ReferenceRow, size: ShortSize) => {
    const { live } = liveDeployments(family, reference);
    if (!live.length)
      return (
        <span className="text-text-dim">
          {reference.currentVersionHash ? 'no live code cell' : 'retired'}
        </span>
      );
    const [first] = live;
    // the out point and how many more live code cells there are: one figure, on one line; "no
    // live code cell" above is words and breaks between them (a list table's rule)
    return (
      <span className="whitespace-nowrap">
        <Link href={`/cell/${first.codeCellTxHash}-${first.codeCellOutputIndex}`} look="inline">
          <ShortId value={first.codeCellTxHash} kind="hash" size={size} />
          <OutpointIndex index={first.codeCellOutputIndex} />
        </Link>
        {live.length > 1 && <span className="text-text-dim"> +{live.length - 1}</span>}
      </span>
    );
  };
  const holder = (reference: ReferenceRow) =>
    reference.custody ? scriptClassLabel(reference.custody.class) : 'Evidence unavailable';
  const title = (reference: ReferenceRow) => {
    const { live } = liveDeployments(family, reference);
    return [
      `${reference.hashType} · ${reference.hashType === 'type' ? 'follows every upgrade' : 'pins this exact binary'}`,
      `code hash ${reference.referenceHash}`,
      `current code ${reference.currentVersionHash ?? 'retired'}`,
      formatCount(reference.liveCellsCount, 'live cell'),
      live.length
        ? `cell dep ${live.map((d) => `${d.codeCellTxHash}:${d.codeCellOutputIndex}`).join(', ')}`
        : 'no live code cell',
      `held by: ${holder(reference)}`,
    ].join(' · ');
  };
  const columns: readonly ListColumn<ReferenceRow>[] = [
    {
      key: 'hashType',
      label: 'hash type',
      // the head is wider than any value ("data1", 5 characters at 12 px)
      minWidth: headBudget('hash type'),
      render: (r) => <span className="text-text-bright">{r.hashType}</span>,
    },
    {
      key: 'codeHash',
      label: 'code hash',
      // the row size, 17 characters at 13 px (132.6), and the gap (readability F3-e: it was 140,
      // the hash at 12 px)
      minWidth: columnPx(small(ROW_HASH) + CELL_GAP_PX),
      render: (r) => <ShortId value={r.referenceHash} kind="hash" size="row" />,
    },
    {
      key: 'cells',
      label: 'live cells',
      // the head, wider than a reference's live cells, seven digits at 13 px ("9,999,999", 70.2:
      // the default lock's family, the largest, holds under two million)
      minWidth: headBudget('live cells'),
      numeric: true,
      render: (r) => r.liveCellsCount.toLocaleString('en-US'),
    },
    {
      key: 'dep',
      label: 'cell dep',
      // The code cell's out point at the row size, an index of two digits, and " +9" more live
      // cells (23 characters at 13 px, 179.4), and the gap (readability F3-e: it was 154, the out
      // point at 12 px without its count).
      minWidth: columnPx(small(`${CODE_CELL_OUTPOINT} +9`) + CELL_GAP_PX),
      render: (r) => depOf(r, 'row'),
    },
    {
      key: 'holder',
      label: 'held by',
      // The table's flexible column, the last (readability F3-e): its head and nine characters of
      // the holder's words at 13 px (70.2) at its minimum, the rest of the container beyond the
      // sum, so the table, 585 px, stays a table in the 588 px of tier 2's right column at 1280.
      // Its words end in an ellipsis and stand whole in the row's title and in "Each reference in
      // full". It was 124.
      minWidth: columnPx(Math.max(meta('held by'), small('x'.repeat(9)))),
      width: 'auto',
      render: (r) => <span className="block truncate">{holder(r)}</span>,
    },
  ];
  return (
    <div className="text-small min-w-0 space-y-4">
      <p className="max-w-measure m-0 min-w-0 [overflow-wrap:anywhere]">
        <ScriptCellsSummary family={family} />
      </p>
      <ScriptByteMap kind={kind} script={null} />
      {references.length > 0 && (
        <>
          <ListTable
            label="Registered references"
            columns={columns.map((c) => ({
              ...c,
              header: <span className="text-meta whitespace-nowrap">{c.label}</span>,
            }))}
            rows={references}
            rowKey={(r) => `${r.hashType}:${r.referenceHash}`}
            density="row"
            rowTitle={title}
            mobileRow={(r) => (
              <PhoneRow
                lines={[
                  {
                    key: 'ref',
                    tone: 'head',
                    left: (
                      <>
                        {r.hashType} · <ShortId value={r.referenceHash} kind="hash" size="tight" />
                      </>
                    ),
                    right: formatCount(r.liveCellsCount, 'live cell'),
                  },
                  { key: 'dep', tone: 'body', left: depOf(r, 'tight'), right: holder(r) },
                ]}
              />
            )}
          />
          {references.some((r) => r.hashType === 'type') && (
            <p className="text-text max-w-measure m-0">
              A type reference follows every upgrade; its data hash with hash type data1 or data2
              pins this exact binary; an upgrade will not reach cells that pin it.
            </p>
          )}
          {/* the library's inner disclosure (lane cd-fixes-controls, the disclosure sweep) */}
          <SectionDisclosure form="inner" summary="Each reference in full">
            <div className="space-y-3">
              {references.map((reference) => (
                <ReferenceFacts
                  key={`${reference.hashType}:${reference.referenceHash}`}
                  family={family}
                  reference={reference}
                />
              ))}
            </div>
          </SectionDisclosure>
        </>
      )}
    </div>
  );
}

/** A column head at 11 px (T5). */
function Head({ label }: { label: string }) {
  return <span className="text-meta">{label}</span>;
}

/** A tier-1 heading row: the h2 as wide as its words, 18 px above its evidence (R6). */
const HEAD_ROW = 'mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1';

/**
 * A tier-1 section (hierarchy R6): a command line (rubbing × Song print, phase 5) at section size,
 * 18 px above its evidence. The section keeps its own name (`label`) for the region.
 */
function Section({
  label,
  command,
  args,
  note,
  className,
  children,
}: {
  label: string;
  command: string;
  /** What the command runs on: a count, a state. */
  args?: ReactNode;
  /** A quiet word on the heading's row, right (the board's "newest first"). */
  note?: string;
  className: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={label} className={`min-w-0 ${className}`}>
      {/* A note brings PromptHeading's own row (the note at its right end), so the wrapper
          only spaces it. */}
      <div className={note === undefined ? HEAD_ROW : 'mb-[18px]'}>
        <PromptHeading command={command} args={args} note={note} />
      </div>
      {children}
    </section>
  );
}

export function ScriptFamilyPage({
  family,
  deprecated,
  chart,
  chartLoading,
  chartError,
  capacityRange,
  onCapacityRange,
  versionUsage,
  codeCells,
  newest,
  cells,
}: ScriptFamilyPageProps) {
  // C1: the page's own header (not DetailLayout) names the tab.
  useDocumentTitle([family.name, 'script']);
  // One hover state for the whole history: the picture and its reading.
  const [active, setActive] = useState<number | null>(null);
  // PA-18: one range control, right over the picture it redraws: since R7, at every width, in the
  // History section's heading row.
  const range = (
    <RangeSelect
      label="range"
      options={CAPACITY_RANGE_OPTIONS}
      value={capacityRange}
      onChange={(next) => {
        // Another range draws other days: a hovered day of the old one lets go.
        setActive(null);
        onCapacityRange(next);
      }}
    />
  );
  if (!family.references || !family.history)
    throw new Error(`/scripts/${family.name} lacks references or history`);
  const history = family.history;
  // The family's cells, oldest first: the pager's pages mode, null for one page, an empty page
  // past the end its place and a way back. Its pages merge its references' reads, which count no
  // rows of their own: no count (the family's live cells is another read, at another tip, and its
  // head states it: lane c2-finish, 1c).
  const cellsAt = cells.read && rankedPageAt('cells', cells.read, cells.walk);
  const cellRows = cells.read?.data ?? [];
  const numbers = versionNumbers(family);
  const lineage = typeLineage(history);
  const holders = lineage ? codeHolders(lineage, history, numbers) : [];
  // Code no one can replace (pinned by data hash, or in a code cell no one can spend) is said in
  // words, never drawn as a line of holders.
  const immutable =
    (family.codeClasses ?? []).length > 0 &&
    (family.codeClasses ?? []).every((row) => isImmutableClass(row.class));
  const deploymentOf = new Map(
    orderedVersions(family).flatMap((version) =>
      version.deployments.map(
        (deployment) =>
          [`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`, version] as const
      )
    )
  );
  // Newest first (board ScriptDetail): the version cells run now leads.
  const rows = [...orderedVersions(family)].reverse().map((version) => ({
    version,
    number: numbers.get(version.versionHash)!,
    current: version.deployments.some((deployment) => deployment.status === 'live'),
  }));
  type Row = (typeof rows)[number];
  type Deployment = Row['version']['deployments'][number];
  const outpointKey = (d: Deployment) => `${d.codeCellTxHash}:${d.codeCellOutputIndex}`;
  // a code cell's out point, at the form's size (U-09): row in the table, tight in the phone row
  const outpointLink = (d: Deployment, size: ShortSize) => (
    <Link href={`/cell/${d.codeCellTxHash}-${d.codeCellOutputIndex}`} look="inline">
      <ShortId value={d.codeCellTxHash} kind="hash" size={size} />
      <OutpointIndex index={d.codeCellOutputIndex} />
    </Link>
  );
  /** A code cell's fate: live, or consumed — by the version that replaced it when one did. */
  const deploymentStatus = (d: Deployment, size: ShortSize): string => {
    const successor = d.consumedByTx
      ? [...deploymentOf].find(([outpoint]) => outpoint.startsWith(`${d.consumedByTx}:`))?.[1]
      : undefined;
    return d.status === 'live'
      ? 'live'
      : successor
        ? `consumed by v${numbers.get(successor.versionHash)}`
        : d.consumedByTx
          ? `consumed in ${shortenHash(d.consumedByTx, size)}`
          : 'consumed';
  };
  const binarySize = (d: Deployment): ReactNode => {
    const state = codeCells.get(outpointKey(d));
    return state?.cell ? (
      `${state.cell.dataSize.toLocaleString('en-US')} B`
    ) : state?.error ? (
      <span role="alert" title={state.error.message} className="text-rouge">
        unavailable
      </span>
    ) : (
      <span className="text-text-dim">…</span>
    );
  };
  /**
   * Who signed a deployment (or that it was the first), and to whom it handed the code cell; its
   * holders at `size` (U-09): row in a title and the table, tight in the phone row.
   */
  const signer = (
    d: Deployment,
    size: ShortSize
  ): { who: string; note: string; marked: boolean } => {
    const marker = history.find(
      (m) =>
        m.event === 'deployed' &&
        m.codeCellTxHash === d.codeCellTxHash &&
        m.codeCellOutputIndex === d.codeCellOutputIndex
    );
    if (!marker)
      return {
        who: d.custody ? `held by ${holderName(d.custody, size)}` : 'custody unavailable',
        note: '',
        marked: false,
      };
    const by = signerOf(marker, history);
    return {
      who: by ? holderName(by, size) : 'first deployed',
      note: !by
        ? `into ${holderName(marker.custody, size)}`
        : holderName(by, size) !== holderName(marker.custody, size)
          ? `hands it to ${marker.custody.lockArgs ? shortenHash(marker.custody.lockArgs, size) : holderName(marker.custody, size)}`
          : '',
      marked: true,
    };
  };
  const signedText = (d: Deployment, size: ShortSize) => {
    const { who, note } = signer(d, size);
    return note ? `${who} ${note}` : who;
  };
  const versionCell = (row: Row) => (
    <span className="text-text-bright">
      v{row.number}
      {/* the version's state, a value of its cell at the cell's 13 px (BRIEF-ink-size-roles item 2) */}
      {row.current && <span className="text-text text-small block">current</span>}
    </span>
  );
  /** The deployment a one-line version row writes: its first; the others are its title's. */
  const leadOf = (row: Row): Deployment | undefined =>
    [...row.version.deployments].sort((a, b) => a.deployedAt - b.deployedAt)[0];
  /**
   * "replaced at #17,894,900": what became of a version no live code cell holds, as sentence parts
   * (lane L: the row links the block; the title reads the same words as text).
   */
  const replacedParts = (d: Deployment | undefined): SentencePart[] =>
    !d
      ? ['no code cell observed']
      : d.consumedAtBlock !== undefined && d.consumedAtBlock !== null
        ? ['replaced at ', { block: d.consumedAtBlock }]
        : d.status === 'consumed'
          ? [deploymentStatus(d, 'row')]
          : ['status unavailable'];
  const replacedText = (d: Deployment | undefined): string => sentenceText(replacedParts(d));
  /** Who signed a deployment, for the title: "signed by X, hands it to Y" or "first deployed into Y". */
  const signerTitle = (d: Deployment): string => {
    const { who, note, marked } = signer(d, 'row');
    if (!marked) return who;
    if (who === 'first deployed') return `${who} ${note}`;
    return note ? `signed by ${who}, ${note}` : `signed by ${who}`;
  };
  /**
   * A version row's hover (L1-4): every field that left the line — the exact time, each code cell
   * with its binary and CKB, who held and who signed it, its fate, and the data hash.
   */
  const versionTitle = (row: Row): string => {
    const deployments = [...row.version.deployments].sort((a, b) => a.deployedAt - b.deployedAt);
    return [
      `v${row.number}`,
      row.current ? 'live · current' : replacedText(leadOf(row)),
      ...(deployments.length ? [] : ['no deployment observed']),
      ...deployments.flatMap((d) => {
        const state = codeCells.get(outpointKey(d));
        return [
          `deployed ${formatUtc8DateTime(d.deployedAt).slice(0, 16)} (UTC+8)${d.createdAtBlock !== undefined ? ` at ${formatBlockNumber(d.createdAtBlock)}` : ''}`,
          `code cell ${outpointKey(d)}`,
          state?.cell
            ? `${state.cell.dataSize.toLocaleString('en-US')} B binary`
            : state?.error
              ? `binary unavailable: ${state.error.message}`
              : 'binary loading',
          ...(state?.cell ? [`${formatCkbAmount(state.cell.capacity).full} CKB`] : []),
          d.custody ? `held by ${holderName(d.custody, 'row')}` : 'custody unavailable',
          signerTitle(d),
          deploymentStatus(d, 'row'),
        ];
      }),
      `data hash ${row.version.versionHash}`,
    ].join(' · ');
  };
  /** One 36 px line (board: 36): the row density's 11 px padding, 2 px less. */
  const ROW = 'py-[9px]';
  const columns: ListColumn<Row>[] = [
    {
      key: 'version',
      label: 'Version',
      header: <Head label="Version" />,
      // its head (7 characters at 12 px, 50.4), wider than "v99" at 14 px, and the gap (F3-e)
      minWidth: columnPx(
        Math.max(meta('Version'), textPx(VERSION_NUMBER.length, fontPx('body'))) + CELL_GAP_PX
      ),
      className: ROW,
      render: (row) => {
        const lead = leadOf(row);
        const label = `v${row.number}`;
        return lead ? (
          <Link
            href={`/cell/${lead.codeCellTxHash}-${lead.codeCellOutputIndex}`}
            look="key"
            className="text-body leading-row"
          >
            {label}
          </Link>
        ) : (
          <span className="text-body text-text-bright leading-row">{label}</span>
        );
      },
    },
    {
      key: 'deployed',
      label: 'Deployed',
      header: <Head label="Deployed" />,
      // the date (10 characters at 13 px, 78) and the gap (F3-e: it was 110)
      minWidth: columnPx(small(formatUtc8Date(0)) + CELL_GAP_PX),
      className: ROW,
      render: (row) => {
        const lead = leadOf(row);
        return lead ? (
          <span className="tabular-nums">
            <Utc8Date timestamp={lead.deployedAt} />
          </span>
        ) : (
          // a cell's content, the reading ink (BRIEF-ink-size-roles item 1)
          <span>—</span>
        );
      },
    },
    {
      key: 'block',
      label: 'Block',
      header: <Head label="Block" />,
      // "#99,999,999" (11 characters at 13 px, 85.8) and the gap (F3-e: it was 120)
      minWidth: columnPx(small(WIDEST_BLOCK_NUMBER) + CELL_GAP_PX),
      numeric: true,
      className: ROW,
      render: (row) => {
        const block = leadOf(row)?.createdAtBlock;
        return block !== undefined ? (
          <Link href={`/blocks/${block}`} look="inline">
            {formatBlockNumber(block)}
          </Link>
        ) : (
          // a cell's content, the reading ink (BRIEF-ink-size-roles item 1)
          <span>—</span>
        );
      },
    },
    {
      key: 'cell',
      label: 'Code cell',
      header: <Head label="Code cell" />,
      // The out point at the row size (20 characters at 13 px, 156), its 8 px gap and a "+9" tag
      // (its words at 12 px and the tag's 12 px of sides and border), and the gap (F3-e: it was 160
      // and 22 % of the table).
      minWidth: columnPx(small(CODE_CELL_OUTPOINT) + 8 + meta('+9') + 12 + CELL_GAP_PX),
      className: ROW,
      render: (row) => {
        const lead = leadOf(row);
        // a cell's content, the reading ink (BRIEF-ink-size-roles item 1)
        if (!lead) return <span>No deployment observed</span>;
        const more = row.version.deployments.filter((d) => d !== lead);
        return (
          <span title={outpointKey(lead)} className="flex min-w-0 items-center gap-2">
            <Link
              href={`/cell/${lead.codeCellTxHash}-${lead.codeCellOutputIndex}`}
              look="inline"
              className="truncate"
            >
              <ShortId value={lead.codeCellTxHash} kind="hash" size="row" />
              <OutpointIndex index={lead.codeCellOutputIndex} />
            </Link>
            {more.length > 0 && (
              <RowTag title={more.map(outpointKey).join(', ')}>+{more.length}</RowTag>
            )}
          </span>
        );
      },
    },
    {
      key: 'held',
      label: 'Held by',
      header: <Head label="Held by" />,
      // The column that takes the free width (F3-e): at its minimum a holder by its kind and its
      // tight id, "multisig 0x1234…abcd" (20 characters at 13 px, 156), and the gap; a longer one
      // ("no one (pinned by data hash)") ends in the one ellipsis, the row's title holding it.
      // It was 190.
      minWidth: columnPx(
        small(`multisig ${'x'.repeat(shortChars(SHORT_FORMS.tight.hash))}`) + CELL_GAP_PX
      ),
      width: 'auto',
      className: `${ROW} truncate`,
      render: (row) => {
        const custody = leadOf(row)?.custody;
        // the tight size, not the row's: with the code cell at the row size a version keeps the
        // row's 90 visible characters (table-rows T6) only so (86; 92 at the row size)
        return custody ? (
          <span>{holderName(custody, 'tight')}</span>
        ) : (
          // a cell's content, the reading ink (BRIEF-ink-size-roles item 1)
          <span>custody unavailable</span>
        );
      },
    },
    {
      key: 'status',
      label: 'Status',
      header: <Head label="Status" />,
      // "consumed in" and the replacing transaction at the row size (29 characters at 13 px,
      // 226.2), wider than "replaced at #99,999,999" and the "live · current" tag, and the
      // gap (F3-e: it was 200 and 18 % of the table).
      minWidth: columnPx(small(`consumed in ${ROW_HASH}`) + CELL_GAP_PX),
      className: `${ROW} truncate`,
      render: (row) =>
        row.current ? (
          <RowTag variant="accent">live · current</RowTag>
        ) : (
          <span className="text-text">
            {/* Lane L: the replacing block is a link. */}
            <SentenceNodes parts={replacedParts(leadOf(row))} />
          </span>
        ),
    },
    {
      key: 'usage',
      label: 'Live cells',
      header: <Head label="Live cells" />,
      // eight digits of live cells, or "unavailable" (11 characters at 13 px, 85.8, the wider); the
      // last column, no gap after it (F3-e: it was 100)
      minWidth: columnPx(Math.max(small(WIDEST_ROW_COUNT), small(UNAVAILABLE))),
      numeric: true,
      className: ROW,
      render: (row) => versionUsage(row.version.versionHash),
    },
  ];
  /** The narrow table keeps its stacked cells: the default density's padding, top-aligned. */
  const NARROW = 'py-3 align-top';
  /** Board L7's 640 table: five columns, the version hash the code cell's second line. */
  const narrowColumns: ListColumn<Row>[] = [
    {
      key: 'version',
      label: 'v',
      // "v99", and "current" under it at the cell's 13 px (54.6, the wider), and the gap: 70.6
      // (F3-e: it was 64; 66.4 with "current" at 12 px, BRIEF-ink-size-roles item 2)
      minWidth: columnPx(Math.max(small(VERSION_NUMBER), small('current')) + CELL_GAP_PX),
      className: NARROW,
      render: versionCell,
    },
    {
      key: 'deployed',
      label: 'Deployed',
      // the date, and the block link under it at the cell's 13 px (85.8, the wider), and the gap:
      // 101.8 (95.2 with the link at 12 px, BRIEF-ink-size-roles item 2: a link is never the meta size)
      minWidth: columnPx(
        Math.max(small(formatUtc8Date(0)), small(WIDEST_BLOCK_NUMBER)) + CELL_GAP_PX
      ),
      className: NARROW,
      render: (row) =>
        row.version.deployments.length ? (
          row.version.deployments.map((d) => (
            <span key={outpointKey(d)} className="block">
              <Utc8Date timestamp={d.deployedAt} />
              {d.createdAtBlock !== undefined && (
                // a link is never the meta size (BRIEF-ink-size-roles item 2)
                <span className="text-small block">{blockLink(d.createdAtBlock)}</span>
              )}
            </span>
          ))
        ) : (
          // a cell's content, the reading ink (BRIEF-ink-size-roles item 1)
          <span>No deployment observed</span>
        ),
    },
    {
      key: 'cell',
      label: 'Code cell · version hash',
      // The column that takes the free width (the board's 1fr). At its minimum the out point at
      // the row size (20 characters at 13 px, 156) and the gap; its notes break between their words
      // and the version hash anywhere under it (F3-e: it was 228).
      minWidth: columnPx(small(CODE_CELL_OUTPOINT) + CELL_GAP_PX),
      className: NARROW,
      width: 'auto',
      render: (row) => (
        <>
          {row.version.deployments.map((d) => (
            <span key={outpointKey(d)} className="block">
              {outpointLink(d, 'row')}
              {/* values of the cell, the reading ink at its 13 px (BRIEF-ink-size-roles items 1 and 2) */}
              <span className="text-small block">
                {binarySize(d)} · {signedText(d, 'row')}
              </span>
            </span>
          ))}
          {/* an id in a cell, the reading ink at its 13 px (BRIEF-ink-size-roles items 1 and 2) */}
          <span className="text-small block [overflow-wrap:anywhere]">
            {row.version.versionHash}
          </span>
        </>
      ),
    },
    {
      key: 'status',
      label: 'Status',
      // A status breaks before the transaction it names; that hash at the row size is one unit (17
      // characters at 13 px, 132.6), and the gap (F3-e: it was 112).
      minWidth: columnPx(small(ROW_HASH) + CELL_GAP_PX),
      className: `text-text ${NARROW}`,
      render: (row) =>
        row.version.deployments.map((d) => (
          <span key={outpointKey(d)} className="block">
            {deploymentStatus(d, 'row')}
          </span>
        )),
    },
    {
      key: 'usage',
      label: 'Live cells',
      // as in the wide table, the last column (F3-e: it was 80)
      minWidth: columnPx(Math.max(small(WIDEST_ROW_COUNT), small(UNAVAILABLE))),
      className: NARROW,
      numeric: true,
      render: (row) => versionUsage(row.version.versionHash),
    },
  ];
  /**
   * A version on a phone (board L7): version, date, code cell and status on one line; the version
   * hash on its own line (it is what a developer copies); each further deployment and the cells
   * using it after.
   */
  const versionPhoneRow = (row: Row) => {
    const [first, ...again] = row.version.deployments;
    const detail = (d: Deployment) =>
      [
        d.createdAtBlock !== undefined ? blockLink(d.createdAtBlock) : null,
        binarySize(d),
        codeCells.get(outpointKey(d))?.cell ? (
          <CkbAmount
            form="summary"
            shannons={BigInt(codeCells.get(outpointKey(d))!.cell!.capacity)}
            signed={false}
            tone="plain"
          />
        ) : null,
        signedText(d, 'tight'),
      ].filter((part) => part !== null);
    const joined = (parts: ReactNode[]) =>
      parts.map((part, i) => (
        <span key={i}>
          {i > 0 && ' · '}
          {part}
        </span>
      ));
    return (
      <PhoneRow
        testId="code-version-row-mobile"
        lines={[
          {
            key: 'version',
            tone: 'body',
            left: (
              <>
                <span className="text-text-bright font-medium">v{row.number}</span>
                {row.current && <span className="text-text"> current</span>}{' '}
                {first ? (
                  <>
                    <span className="text-text">
                      <Utc8Date timestamp={first.deployedAt} />
                    </span>{' '}
                    · {outpointLink(first, 'tight')}
                  </>
                ) : (
                  <span className="text-text-dim">no deployment observed</span>
                )}
              </>
            ),
            right: first ? (
              <span className="text-text-dim">{deploymentStatus(first, 'tight')}</span>
            ) : null,
          },
          {
            key: 'hash',
            tone: 'quiet',
            left: row.version.versionHash,
          },
          {
            key: 'detail',
            tone: 'quiet',
            left: first ? joined(detail(first)) : null,
            right: <>live cells using it: {versionUsage(row.version.versionHash)}</>,
          },
          ...again.map((d) => ({
            key: `again-${outpointKey(d)}`,
            tone: 'quiet' as const,
            left: joined([
              <>
                deployed again <Utc8Date timestamp={d.deployedAt} />
              </>,
              outpointLink(d, 'tight'),
              ...detail(d),
            ]),
            right: deploymentStatus(d, 'tight'),
          })),
        ]}
      />
    );
  };
  const typeReference = lineage?.[0]?.referenceHash;
  const roleCrumb =
    family.scriptKind === 'lock' ? 'Locks' : family.scriptKind === 'type' ? 'Types' : null;
  const binaries = typeReference
    ? `one type id, ${shortenHash(typeReference, 'row')}, ${formatCount(rows.length, 'binary', 'binaries')}`
    : formatCount(rows.length, 'binary', 'binaries');
  return (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">
        <div className="text-body min-w-0 font-mono">
          {/* Hierarchy v3 (R13, R6): the hero, its reading and figures; sections 64 / 72 px apart. */}
          <div data-tier="1" className="min-w-0">
            {/* the chart pages' breadcrumb (lane D1): its links inline in the reading ink, the
                separator quiet */}
            <nav aria-label="Breadcrumb" className="text-small mb-2">
              <Link href="/scripts" look="inline">
                Scripts
              </Link>
              {roleCrumb && (
                <>
                  {' '}
                  <span className="text-text-ghost">/</span> {roleCrumb}
                </>
              )}
            </nav>
            <section aria-label={family.name} className="min-w-0">
              <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
                <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
                  {/* PB-06 / PA-32: an entity h1, one type step smaller below md (30 → 26). */}
                  <h1 className="text-text-bright text-entity max-md:text-index break-words font-medium">
                    {family.name}
                  </h1>
                  <ScriptHeading family={family} />
                  {deprecated && (
                    // Lane F follow-up 2: the warning a user must not miss (was "· Deprecated", dim).
                    <RowTag variant="warn" title={DEPRECATED_TITLE}>
                      deprecated
                    </RowTag>
                  )}
                </div>
              </div>
              {(family.description || family.website) && (
                <p
                  data-role="lede"
                  className="text-text text-body max-w-measure mt-1.5 [overflow-wrap:anywhere]"
                >
                  {family.description}
                  {family.website && (
                    <>
                      {family.description ? ' · ' : ''}
                      <ExternalLink
                        href={family.website}
                        look="inline"
                        className="whitespace-nowrap"
                      >
                        {family.website.replace(/^https?:\/\//, '').replace(/\/$/, '')}
                      </ExternalLink>
                    </>
                  )}
                </p>
              )}
              <ScriptHeroFigure family={family} />
            </section>
            <ScriptHeroMetrics family={family} lineage={lineage} />
            {/* Readability R7: the history after the hero's figures, 160 px under one sentence its
                points give, its range in its own heading row at every width (it was the hero's
                300 px picture, the range at the end of the title row from md) */}
            <div role="region" aria-label="History" className="mt-[72px] min-w-0">
              <DetailSection title="History" actions={range}>
                {chartLoading ? (
                  <p className="text-text-dim text-small py-6">Loading capacity history…</p>
                ) : chartError ? (
                  <p role="alert" className="text-negative text-small max-w-measure">
                    Capacity history unavailable: {chartError.message}
                  </p>
                ) : chart?.data.length ? (
                  <>
                    <ScriptHistoryFigure
                      family={family}
                      chart={chart}
                      lineage={lineage}
                      numbers={numbers}
                      active={active}
                      onActive={setActive}
                    />
                    <ScriptHistoryReading
                      chart={chart}
                      lineage={lineage}
                      numbers={numbers}
                      active={active}
                    />
                  </>
                ) : (
                  // an empty-state sentence, the reading ink (BRIEF-ink-size-roles item 3)
                  <p className="text-small max-w-measure py-6">No capacity history.</p>
                )}
              </DetailSection>
            </div>
            {/* Lane S: what the script holds now, before how its code can change. */}
            <ScriptNewestCells family={family} newest={newest} className="mt-16" />
            {/* A17: the holders the timeline draws (each run of the lineage's code cell, a
                holder that comes back counting again, as the timeline's own count does), or
                code no one can replace; several references with no single lineage run on
                nothing (the figure says it in words). */}
            <Section
              label="Who can change its code"
              command="Who can change its code"
              args={
                immutable
                  ? 'immutable'
                  : lineage && holders.length
                    ? formatCount(holders.length, 'owner')
                    : undefined
              }
              className="mt-[72px]"
            >
              {lineage && holders.length && !immutable ? (
                // T8: one Type ID lineage is a holder timeline.
                <HolderTimeline
                  family={family}
                  lineage={lineage}
                  holders={holders}
                  numbers={numbers}
                  now={Date.now()}
                />
              ) : (
                // Immutable code, or several references with no single lineage: said in words.
                <ScriptCustodyFigure family={family} lineage={null} holders={[]} />
              )}
            </Section>
            <Section
              label="Code versions"
              command="Versions"
              args={rows.length.toLocaleString('en-US')}
              note="newest first"
              className="mt-[72px]"
            >
              <ListTable
                label="Code versions"
                rows={rows}
                rowKey={(row) => row.version.versionHash}
                density="row"
                rowTitle={versionTitle}
                // T5: body ink for the row, the version alone bright.
                rowClassName={() => 'text-text'}
                columns={columns}
                narrowColumns={narrowColumns}
                mobileRow={versionPhoneRow}
              />
            </Section>
            <section
              id="its-cells"
              aria-label="Its cells"
              className="mt-[72px] min-w-0 scroll-mt-4"
            >
              <div className={HEAD_ROW}>
                <PromptHeading
                  command="Cells"
                  args={`${family.liveCellsCount.toLocaleString('en-US')} live`}
                />
              </div>
              <div className="text-text-dim text-meta mb-3 flex flex-wrap gap-x-4 gap-y-1">
                {ANATOMY_LEGEND.map(([name, color]) => (
                  <span key={name} className="inline-flex items-center gap-1.5">
                    <span
                      aria-hidden="true"
                      className="inline-block h-2 w-2"
                      style={{ backgroundColor: color }}
                    />
                    {name}
                  </span>
                ))}
              </div>
              {cells.loading ? (
                <p className="text-text-dim text-small py-6">Loading cells…</p>
              ) : (
                <ListTable
                  label="Its cells"
                  rows={cellRows}
                  rowKey={(cell) => `${cell.txHash}:${cell.outputIndex}`}
                  empty={`No live cells use ${family.name}`}
                  // T5: body ink for the row, the cell alone bright.
                  rowClassName={() => 'text-text'}
                  columns={[
                    {
                      key: 'cell',
                      label: 'Cell',
                      header: <Head label="Cell" />,
                      // The out point at the row size and an index of four digits (22
                      // characters at the key's 14 px, 184.8), and the gap (readability F3-e: it
                      // was 176, the out point at 13 px with a one-digit index, and 15 %).
                      minWidth: columnPx(
                        textPx(CELL_OUTPOINT.length, fontPx('body')) + CELL_GAP_PX
                      ),
                      render: (cell) => (
                        <Link
                          href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                          look="key"
                          className="text-body leading-row"
                        >
                          <ShortId value={cell.txHash} kind="hash" size="row" />
                          <OutpointIndex index={cell.outputIndex} />
                        </Link>
                      ),
                    },
                    {
                      key: 'owner',
                      label: 'Owner',
                      header: <Head label="Owner" />,
                      // the address at the row size (13 characters at 13 px, 101.4) and the gap
                      // (F3-e: it was 170 and 16 %)
                      minWidth: columnPx(
                        textPx(shortChars(SHORT_FORMS.row.address), fontPx('small')) + CELL_GAP_PX
                      ),
                      render: (cell) => <CellOwner cell={cell} size="row" />,
                    },
                    {
                      key: 'type',
                      label: 'Also guarded by',
                      header: <Head label="Also guarded by" />,
                      // The table's flexible column (F3-e): a type's code hash at the row size
                      // (17 characters at 13 px, 132.6) and the gap at its minimum; a registry
                      // name takes the rest of the container, breaking between its words. It
                      // was 140 and 13 %.
                      minWidth: columnPx(small(ROW_HASH) + CELL_GAP_PX),
                      width: 'auto',
                      render: (cell) => <AlsoGuardedBy cell={cell} size="row" />,
                    },
                    {
                      key: 'anatomy',
                      label: 'What it holds',
                      header: <Head label="What it holds" />,
                      // The bar, no shorter than a bar's track (48), or "Cell anatomy
                      // unavailable" breaking between its words ("unavailable", 85.8, the wider),
                      // and the gap (F3-e: it was 150 and 14 %).
                      minWidth: columnPx(Math.max(MIN_TRACK_PX, small(UNAVAILABLE)) + CELL_GAP_PX),
                      render: (cell) => <CellAnatomy cell={cell} />,
                    },
                    {
                      key: 'capacity',
                      label: 'Capacity',
                      header: <Head label="Capacity" />,
                      // A cell's capacity under ten billion CKB, every decimal (22 characters at
                      // 13 px, 171.6), its unit " CKB" at 12 px (28.8) and the gap (F3-e: it was
                      // 180 and 17 %).
                      minWidth: columnPx(small(WIDEST_CELL_CAPACITY) + meta(' CKB') + CELL_GAP_PX),
                      numeric: true,
                      render: (cell) => (
                        <CkbAmount
                          form="column"
                          shannons={BigInt(cell.capacity)}
                          signed={false}
                          tone="plain"
                        />
                      ),
                    },
                    {
                      key: 'occupied',
                      label: 'Occupied',
                      header: <Head label="Occupied" />,
                      // occupied bytes under ten million, "9,999,999 B", or "unavailable" (85.8
                      // each), and the gap (F3-e: it was 90 and 9 %)
                      minWidth: columnPx(
                        Math.max(small(WIDEST_OCCUPIED), small(UNAVAILABLE)) + CELL_GAP_PX
                      ),
                      numeric: true,
                      render: (cell) => <OccupiedBytes cell={cell} />,
                    },
                    {
                      key: 'created',
                      label: 'Created',
                      header: <Head label="Created" />,
                      // "#99,999,999 · 2026-11-02" (24 characters at 13 px, 187.2), the last
                      // column, no gap after it (F3-e: it was 170 and 16 %)
                      minWidth: columnPx(small(`${WIDEST_BLOCK_NUMBER} · ${formatUtc8Date(0)}`)),
                      numeric: true,
                      render: (cell) => (
                        <span className="text-text">
                          {blockLink(cell.createdAtBlock)}
                          {unitNode(
                            cell.createdTimestamp && ` · ${formatUtc8Date(cell.createdTimestamp)}`
                          )}
                        </span>
                      ),
                    },
                  ]}
                  mobileRow={(cell) => <ScriptCellPhoneRow cell={cell} />}
                />
              )}
              {cellRows.length > 0 && (
                <p className="text-text text-small max-w-measure mt-2">
                  Oldest first, as the cell index keeps them · {cellRows.length} of{' '}
                  {family.liveCellsCount.toLocaleString('en-US')}
                </p>
              )}
              {cellsAt && <Pager mode="pages" label="cells" {...cellsAt} />}
            </section>
          </div>
          <div id="evidence" className="scroll-mt-4">
            <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
              {/* Board ScriptDetail (table-rows L1-5): one paragraph, then the script's facts. */}
              <DetailSection title="How to read this page">
                <Prose>
                  <p>
                    <ScriptHistoryNotes family={family} chart={chart} lineage={lineage} inline />{' '}
                    <ScriptSummarySentence family={family} lineage={lineage} inline />{' '}
                    <ScriptCustodyMechanism
                      family={family}
                      lineage={lineage}
                      binaries={binaries}
                      inline
                    />
                  </p>
                </Prose>
              </DetailSection>
              <DetailSection title="The script">
                <ScriptFacts family={family} />
              </DetailSection>
            </DetailTier2>
          </div>
        </div>
      </main>
    </div>
  );
}

/**
 * A script's cell on a phone (the phone-rows board): the out point and its capacity; who owns it,
 * the type script that also guards it (none said where it has none) and where it was made, each
 * after its name; the list's one bar, what the cell holds, beside the bytes it occupies.
 */
function ScriptCellPhoneRow({ cell }: { cell: Cell }) {
  return (
    <PhoneRow
      testId="script-cell-row-mobile"
      head={SCRIPT_CELL_PHONE_HEAD_PX}
      lines={[
        {
          key: 'cell',
          tone: 'head',
          left: (
            <Link href={`/cell/${cell.txHash}-${cell.outputIndex}`} look="key">
              <ShortId value={cell.txHash} kind="hash" size="tight" />
              <OutpointIndex index={cell.outputIndex} />
            </Link>
          ),
          right: (
            <CkbAmount
              form="summary"
              shannons={BigInt(cell.capacity)}
              signed={false}
              tone="plain"
            />
          ),
        },
        {
          key: 'held',
          tone: 'body',
          left: (
            <Dotted
              parts={[
                <Labelled key="owner" label="owner">
                  <CellOwner cell={cell} size="tight" />
                </Labelled>,
                cell.typeScriptHash && (
                  <Labelled key="type" label="type">
                    <AlsoGuardedBy cell={cell} size="tight" />
                  </Labelled>
                ),
                <Labelled key="created" label="created">
                  {blockLink(cell.createdAtBlock)}
                </Labelled>,
              ]}
            />
          ),
        },
        {
          key: 'anatomy',
          tone: 'quiet',
          grow: true,
          left: <CellAnatomy cell={cell} className="w-full min-w-12" />,
          right: (
            <Labelled label="occupied" after>
              <OccupiedBytes cell={cell} />
            </Labelled>
          ),
        },
      ]}
    />
  );
}
