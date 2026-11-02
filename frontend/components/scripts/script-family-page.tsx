'use client';
import { useState, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { CellBar } from '@/components/ui/cell-bar';
import { CapacityRangeSelector } from '@/components/ui/capacity-range-selector';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { PhoneRow } from '@/components/ui/phone-row';
import { ExactCkb } from '@/components/activity-event-row';
import { ScriptCustodyEvidence, isImmutableClass } from '@/components/scripts/script-code-evidence';
import { HolderTimeline } from '@/components/scripts/holder-timeline';
import {
  ScriptCellsSummary,
  ScriptCodeTrack,
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
import type { CapacityRangeKey } from '@/lib/capacity-range';
import { formatCapacity, formatCkbAmount, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { cn } from '@/lib/utils';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { RowTag } from '@/components/ui/row-tag';
import {
  codeHolders,
  holderName,
  orderedVersions,
  shortHex,
  signerOf,
  typeLineage,
  versionNumbers,
} from '@/lib/script-lineage';

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
  cells: {
    pageSize: number;
    rows: readonly Cell[];
    loading: boolean;
    page: number;
    hasMore: boolean;
    hasPrevious: boolean;
    currentCount: number;
    onNext: () => void;
    onPrevious: () => void;
  };
}

function CellOwner({ cell }: { cell: Cell }) {
  return cell.address ? (
    <Link href={`/address/${cell.address}`} className="text-text hover:text-jade">
      {cell.address.slice(0, 10)}…{cell.address.slice(-6)}
    </Link>
  ) : (
    <span role="alert" className="text-negative">
      Owner address unavailable
    </span>
  );
}

/** The type script beside the lock: its registry name (R6), or its code hash when unregistered. */
function AlsoGuardedBy({ cell }: { cell: Cell }) {
  if (!cell.typeScriptHash) return <span className="text-[#4b5266]">—</span>;
  if (!cell.typeCodeHash || !cell.typeHashType)
    return (
      <span role="alert" className="text-negative">
        Type reference unavailable
      </span>
    );
  if (cell.typeScriptName === undefined)
    throw new Error(`Cell ${cell.txHash}:${cell.outputIndex} row has no typeScriptName (R6)`);
  return (
    <Link
      href={`/script/${cell.typeCodeHash}?hashType=${cell.typeHashType}`}
      className="hover:text-jade text-[#a0a8be]"
    >
      {cell.typeScriptName ?? shortHex(cell.typeCodeHash)}
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
        className="text-[#a0a8be]"
        title={`Virtual common knowledge size: ${formatCapacity(cell.virtualCommonKnowledgeSize)}`}
      >
        {toBytes(cell.virtualCommonKnowledgeSize)}
      </span>
    );
  }
  return <span className="text-[#a0a8be]">{toBytes(cell.commonKnowledgeSize)}</span>;
}

function CellAnatomy({ cell }: { cell: Cell }) {
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
  ['capacity', '#dee2ec'],
  ['lock', '#7aa7d6'],
  ['type', '#b5c77a'],
  ['data', '#6fc3d9'],
  ['free', '#1a1f30'],
] as const;

type ReferenceRow = NonNullable<ScriptFamilyDetail['references']>[number];

/**
 * Tier 2's "The script" (table-rows L1-5, board ScriptDetail): its cells, then one definition list
 * per registered reference — the reference and how it follows the code, the current code and how to
 * pin it instead, the cell dep that serves it, and the evidence of who holds its code cell. The
 * former "Use it in a transaction" and "Registered references" blocks, fact for fact.
 */
function ScriptFacts({ family }: { family: ScriptFamilyDetail }) {
  const references: ReferenceRow[] = family.references ?? [];
  const term = 'text-text-dim';
  const value = 'm-0 min-w-0 [overflow-wrap:anywhere]';
  return (
    <div className="text-small space-y-5">
      {(references.length ? references : [null]).map((reference, i) => {
        const version = reference
          ? family.versions.find((v) => v.versionHash === reference.currentVersionHash)
          : undefined;
        const live = version?.deployments.filter((d) => d.status === 'live') ?? [];
        return (
          <dl
            key={reference ? `${reference.hashType}:${reference.referenceHash}` : 'cells'}
            className={cn(
              'grid grid-cols-[110px_minmax(0,1fr)] gap-x-3 gap-y-1',
              i > 0 && 'border-base-border-subtle border-t pt-4'
            )}
          >
            {i === 0 && (
              <>
                <dt className={term}>its cells</dt>
                <dd className={value}>
                  <ScriptCellsSummary family={family} />
                </dd>
              </>
            )}
            {reference && (
              <>
                <dt className={term}>code hash</dt>
                <dd className={value}>{reference.referenceHash}</dd>
                <dt className={term}>hash type</dt>
                <dd className={value}>
                  {reference.hashType} ·{' '}
                  {reference.hashType === 'type'
                    ? 'follows every upgrade'
                    : 'pins this exact binary'}{' '}
                  · {formatCount(reference.liveCellsCount, 'live cell')}
                </dd>
                <dt className={term}>current code</dt>
                <dd className={value}>
                  {reference.currentVersionHash ?? 'Retired · no current code cell'}
                </dd>
                {reference.hashType === 'type' && version && (
                  <>
                    <dt className={term}>pin instead</dt>
                    <dd className={cn(value, 'text-text-dim')}>
                      its data hash with hash type data1 or data2 pins this exact binary; an upgrade
                      will not reach cells that pin it
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
                          className="text-text hover:text-jade"
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
              </>
            )}
          </dl>
        );
      })}
    </div>
  );
}

/** A tier-1 section (hierarchy R6): h2 at section size, 18 px above its evidence. */
function Section({
  title,
  note,
  className,
  children,
}: {
  title: string;
  /** A quiet word on the heading's row, right (the board's "newest first"). */
  note?: string;
  className: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className={`min-w-0 ${className}`}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">{title}</h2>
        {note && <span className="text-text-dim text-small">{note}</span>}
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
  cells,
}: ScriptFamilyPageProps) {
  // One hover state for the whole history: the picture, its code track and its reading.
  const [active, setActive] = useState<number | null>(null);
  if (!family.references || !family.history)
    throw new Error(`/scripts/${family.name} lacks references or history`);
  const history = family.history;
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
  const outpointLink = (d: Deployment) => (
    <Link
      href={`/cell/${d.codeCellTxHash}-${d.codeCellOutputIndex}`}
      className="text-text hover:text-jade hover:underline"
    >
      {shortHex(d.codeCellTxHash, 8, 6)}:{d.codeCellOutputIndex}
    </Link>
  );
  /** A code cell's fate: live, or consumed — by the version that replaced it when one did. */
  const deploymentStatus = (d: Deployment): string => {
    const successor = d.consumedByTx
      ? [...deploymentOf].find(([outpoint]) => outpoint.startsWith(`${d.consumedByTx}:`))?.[1]
      : undefined;
    return d.status === 'live'
      ? 'live'
      : successor
        ? `consumed by v${numbers.get(successor.versionHash)}`
        : d.consumedByTx
          ? `consumed in ${shortHex(d.consumedByTx, 8, 6)}`
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
  /** Who signed a deployment (or that it was the first), and to whom it handed the code cell. */
  const signer = (d: Deployment): { who: string; note: string; marked: boolean } => {
    const marker = history.find(
      (m) =>
        m.event === 'deployed' &&
        m.codeCellTxHash === d.codeCellTxHash &&
        m.codeCellOutputIndex === d.codeCellOutputIndex
    );
    if (!marker)
      return {
        who: d.custody ? `held by ${holderName(d.custody)}` : 'custody unavailable',
        note: '',
        marked: false,
      };
    const by = signerOf(marker, history);
    return {
      who: by ? holderName(by) : 'first deployed',
      note: !by
        ? `into ${holderName(marker.custody)}`
        : holderName(by) !== holderName(marker.custody)
          ? `hands it to ${marker.custody.lockArgs ? shortHex(marker.custody.lockArgs) : holderName(marker.custody)}`
          : '',
      marked: true,
    };
  };
  const signedText = (d: Deployment) => {
    const { who, note } = signer(d);
    return note ? `${who} ${note}` : who;
  };
  const versionCell = (row: Row) => (
    <span className="text-text-bright">
      v{row.number}
      {row.current && <span className="text-text text-meta block">current</span>}
    </span>
  );
  /** The deployment a one-line version row writes: its first; the others are its title's. */
  const leadOf = (row: Row): Deployment | undefined =>
    [...row.version.deployments].sort((a, b) => a.deployedAt - b.deployedAt)[0];
  /** "replaced at #17,894,900": what became of a version no live code cell holds. */
  const replacedText = (d: Deployment | undefined): string =>
    !d
      ? 'no code cell observed'
      : d.consumedAtBlock !== undefined && d.consumedAtBlock !== null
        ? `replaced at ${formatBlockNumber(d.consumedAtBlock)}`
        : d.status === 'consumed'
          ? deploymentStatus(d)
          : 'status unavailable';
  /** Who signed a deployment, for the title: "signed by X, hands it to Y" or "first deployed into Y". */
  const signerTitle = (d: Deployment): string => {
    const { who, note, marked } = signer(d);
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
          d.custody ? `held by ${holderName(d.custody)}` : 'custody unavailable',
          signerTitle(d),
          deploymentStatus(d),
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
      minWidth: 64,
      className: ROW,
      render: (row) => {
        const lead = leadOf(row);
        const label = `v${row.number}`;
        return lead ? (
          <Link
            href={`/cell/${lead.codeCellTxHash}-${lead.codeCellOutputIndex}`}
            className="text-body text-text-bright leading-[18px] hover:underline"
          >
            {label}
          </Link>
        ) : (
          <span className="text-body text-text-bright leading-[18px]">{label}</span>
        );
      },
    },
    {
      key: 'deployed',
      label: 'Deployed',
      minWidth: 110,
      className: ROW,
      render: (row) => {
        const lead = leadOf(row);
        return lead ? (
          <span className="tabular-nums">{formatUtc8Date(lead.deployedAt)}</span>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
    },
    {
      key: 'block',
      label: 'Block',
      minWidth: 120,
      numeric: true,
      className: ROW,
      render: (row) => {
        const block = leadOf(row)?.createdAtBlock;
        return block !== undefined ? (
          <Link href={`/blocks/${block}`} className="text-text-dim hover:text-jade">
            {formatBlockNumber(block)}
          </Link>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
    },
    {
      key: 'cell',
      label: 'Code cell',
      minWidth: 160,
      width: '22%',
      className: ROW,
      render: (row) => {
        const lead = leadOf(row);
        if (!lead) return <span className="text-text-dim">No deployment observed</span>;
        const more = row.version.deployments.filter((d) => d !== lead);
        return (
          <span title={outpointKey(lead)} className="flex min-w-0 items-center gap-2">
            <span className="text-text-dim truncate">
              {shortHex(lead.codeCellTxHash, 6, 4)}:{lead.codeCellOutputIndex}
            </span>
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
      minWidth: 190,
      // The column that takes the free width.
      width: 'auto',
      className: `${ROW} truncate`,
      render: (row) => {
        const custody = leadOf(row)?.custody;
        return custody ? (
          <span>{holderName(custody)}</span>
        ) : (
          <span className="text-text-dim">custody unavailable</span>
        );
      },
    },
    {
      key: 'status',
      label: 'Status',
      minWidth: 200,
      width: '18%',
      className: `${ROW} truncate`,
      render: (row) =>
        row.current ? (
          <RowTag variant="accent">live · current</RowTag>
        ) : (
          <span className="text-text-dim">{replacedText(leadOf(row))}</span>
        ),
    },
    {
      key: 'usage',
      label: 'Live cells',
      minWidth: 100,
      numeric: true,
      className: ROW,
      render: (row) => versionUsage(row.version.versionHash),
    },
  ];
  /** The narrow table keeps its stacked cells: the default density's padding, top-aligned. */
  const NARROW = 'py-3 align-top';
  /** Board L7's 640 table: five columns, the version hash the code cell's second line. */
  const narrowColumns: ListColumn<Row>[] = [
    { key: 'version', label: 'v', minWidth: 64, className: NARROW, render: versionCell },
    {
      key: 'deployed',
      label: 'Deployed',
      minWidth: 96,
      className: NARROW,
      render: (row) =>
        row.version.deployments.length ? (
          row.version.deployments.map((d) => (
            <span key={outpointKey(d)} className="block">
              {formatUtc8Date(d.deployedAt)}
              {d.createdAtBlock !== undefined && (
                <span className="text-meta block">{blockLink(d.createdAtBlock)}</span>
              )}
            </span>
          ))
        ) : (
          <span className="text-text-dim">No deployment observed</span>
        ),
    },
    {
      key: 'cell',
      label: 'Code cell · version hash',
      minWidth: 228,
      className: NARROW,
      // The column that takes the free width (the board's 1fr).
      width: 'auto',
      render: (row) => (
        <>
          {row.version.deployments.map((d) => (
            <span key={outpointKey(d)} className="block">
              {outpointLink(d)}
              <span className="text-text-dim text-meta block">
                {binarySize(d)} · {signedText(d)}
              </span>
            </span>
          ))}
          <span className="text-text-dim text-meta block [overflow-wrap:anywhere]">
            {row.version.versionHash}
          </span>
        </>
      ),
    },
    {
      key: 'status',
      label: 'Status',
      minWidth: 112,
      className: `text-text-dim ${NARROW}`,
      render: (row) =>
        row.version.deployments.map((d) => (
          <span key={outpointKey(d)} className="block">
            {deploymentStatus(d)}
          </span>
        )),
    },
    {
      key: 'usage',
      label: 'Live cells',
      minWidth: 80,
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
          <ExactCkb
            shannons={BigInt(codeCells.get(outpointKey(d))!.cell!.capacity)}
            signed={false}
            tone="plain"
          />
        ) : null,
        signedText(d),
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
                    <span className="text-text">{formatUtc8Date(first.deployedAt)}</span> ·{' '}
                    {outpointLink(first)}
                  </>
                ) : (
                  <span className="text-text-dim">no deployment observed</span>
                )}
              </>
            ),
            right: first ? <span className="text-text-dim">{deploymentStatus(first)}</span> : null,
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
              <>deployed again {formatUtc8Date(d.deployedAt)}</>,
              outpointLink(d),
              ...detail(d),
            ]),
            right: deploymentStatus(d),
          })),
        ]}
      />
    );
  };
  const typeReference = lineage?.[0]?.referenceHash;
  const roleCrumb =
    family.scriptKind === 'lock' ? 'Locks' : family.scriptKind === 'type' ? 'Types' : null;
  const binaries = typeReference
    ? `one type id, ${shortHex(typeReference, 8, 6)}, ${rows.length} binaries`
    : `${rows.length} binaries`;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="text-body min-w-0 font-mono">
          {/* Hierarchy v3 (R13, R6): the hero, its reading and figures; sections 64 / 72 px apart. */}
          <div data-tier="1" className="min-w-0">
            <nav aria-label="Breadcrumb" className="text-text-dim text-small mb-2">
              <Link href="/scripts" className="hover:text-text-bright">
                Scripts
              </Link>
              {roleCrumb && (
                <>
                  {' '}
                  <span className="text-[#343c50]">/</span> {roleCrumb}
                </>
              )}
            </nav>
            <section aria-label={family.name} className="min-w-0">
              <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
                <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
                  <h1 className="text-text-bright text-entity break-words font-medium">
                    {family.name}
                  </h1>
                  <ScriptHeading family={family} />
                  {deprecated && <span className="text-text-dim text-small">· Deprecated</span>}
                </div>
                <CapacityRangeSelector
                  value={capacityRange}
                  onChange={(range) => {
                    // Another range draws other days: a hovered day of the old one lets go.
                    setActive(null);
                    onCapacityRange(range);
                  }}
                  quiet
                />
              </div>
              {(family.description || family.website) && (
                <p
                  data-role="lede"
                  className="text-text-dim text-body mt-1.5 max-w-[560px] [overflow-wrap:anywhere]"
                >
                  {family.description}
                  {family.website && (
                    <>
                      {family.description ? ' · ' : ''}
                      <a
                        href={family.website}
                        className="hover:text-text-bright whitespace-nowrap hover:underline"
                      >
                        {family.website.replace(/^https?:\/\//, '').replace(/\/$/, '')} ↗
                      </a>
                    </>
                  )}
                </p>
              )}
              <ScriptHeroFigure family={family} />
              {chartLoading ? (
                <p className="text-text-dim text-small mt-[18px] py-6">Loading capacity history…</p>
              ) : chartError ? (
                <p role="alert" className="text-negative text-small mt-[18px]">
                  Capacity history unavailable: {chartError.message}
                </p>
              ) : chart?.data.length ? (
                <ScriptHistoryFigure
                  family={family}
                  chart={chart}
                  lineage={lineage}
                  numbers={numbers}
                  active={active}
                  onActive={setActive}
                />
              ) : (
                <p className="text-text-dim text-small mt-[18px] py-6">No capacity history yet</p>
              )}
            </section>
            {!chartLoading && !chartError && chart?.data.length ? (
              <>
                <ScriptCodeTrack
                  chart={chart}
                  lineage={lineage}
                  numbers={numbers}
                  holders={holders}
                />
                <ScriptHistoryReading
                  chart={chart}
                  lineage={lineage}
                  numbers={numbers}
                  active={active}
                />
              </>
            ) : null}
            <ScriptHeroMetrics family={family} lineage={lineage} />
            <Section title="Who can change its code" className="mt-16">
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
            <Section title="Code versions" note="newest first" className="mt-[72px]">
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
              <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
                <h2 className="text-text-bright text-section font-medium">Its cells</h2>
              </div>
              <div className="text-text-dim text-meta mb-3 flex flex-wrap gap-x-4 gap-y-1">
                {ANATOMY_LEGEND.map(([name, color]) => (
                  <span key={name} className="inline-flex items-center gap-1.5">
                    <span
                      aria-hidden="true"
                      className="inline-block h-2 w-2 rounded-sm"
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
                  rows={cells.rows}
                  rowKey={(cell) => `${cell.txHash}:${cell.outputIndex}`}
                  empty={`No live cells use ${family.name}`}
                  // T5: body ink for the row, the cell alone bright.
                  rowClassName={() => 'text-text'}
                  columns={[
                    {
                      key: 'cell',
                      label: 'Cell',
                      minWidth: 160,
                      width: '15%',
                      render: (cell) => (
                        <Link
                          href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                          className="text-body text-text-bright hover:text-jade leading-[18px] hover:underline"
                        >
                          {shortHex(cell.txHash, 6, 4)}:{cell.outputIndex}
                        </Link>
                      ),
                    },
                    {
                      key: 'owner',
                      label: 'Owner',
                      minWidth: 170,
                      width: '16%',
                      render: (cell) => <CellOwner cell={cell} />,
                    },
                    {
                      key: 'type',
                      label: 'Also guarded by',
                      minWidth: 140,
                      width: '13%',
                      render: (cell) => <AlsoGuardedBy cell={cell} />,
                    },
                    {
                      key: 'anatomy',
                      label: 'What it holds',
                      minWidth: 150,
                      width: '14%',
                      render: (cell) => <CellAnatomy cell={cell} />,
                    },
                    {
                      key: 'capacity',
                      label: 'Capacity',
                      minWidth: 180,
                      width: '17%',
                      numeric: true,
                      render: (cell) => (
                        <ExactCkb shannons={BigInt(cell.capacity)} signed={false} tone="plain" />
                      ),
                    },
                    {
                      key: 'occupied',
                      label: 'Occupied',
                      minWidth: 90,
                      width: '9%',
                      numeric: true,
                      render: (cell) => <OccupiedBytes cell={cell} />,
                    },
                    {
                      key: 'created',
                      label: 'Created',
                      minWidth: 170,
                      width: '16%',
                      numeric: true,
                      render: (cell) => (
                        <span className="text-text-dim">
                          {blockLink(cell.createdAtBlock)}
                          {cell.createdTimestamp && ` · ${formatUtc8Date(cell.createdTimestamp)}`}
                        </span>
                      ),
                    },
                  ]}
                  mobileRow={(cell) => (
                    <div className="text-small min-w-0 space-y-1.5">
                      <Link
                        href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                        className="text-text hover:text-jade"
                      >
                        {shortHex(cell.txHash, 6, 4)}:{cell.outputIndex}
                      </Link>
                      <div>
                        <CellOwner cell={cell} /> · <AlsoGuardedBy cell={cell} />
                      </div>
                      <CellAnatomy cell={cell} />
                      <div className="flex flex-wrap justify-between gap-2">
                        <ExactCkb shannons={BigInt(cell.capacity)} signed={false} tone="plain" />
                        <span>
                          <OccupiedBytes cell={cell} /> occupied
                        </span>
                      </div>
                      <div className="text-text-dim">Created {blockLink(cell.createdAtBlock)}</div>
                    </div>
                  )}
                />
              )}
              {cells.rows.length > 0 && (
                <p className="text-text-dim text-small mt-2">
                  Oldest first, as the cell index keeps them · {cells.rows.length} of{' '}
                  {family.liveCellsCount.toLocaleString('en-US')}
                </p>
              )}
              {cells.rows.length > 0 && (
                <CursorPagination
                  total={family.liveCellsCount}
                  totalLabel="cells"
                  pageSize={cells.pageSize}
                  page={cells.page}
                  currentCount={cells.currentCount}
                  hasMore={cells.hasMore}
                  hasPrevious={cells.hasPrevious}
                  onNext={cells.onNext}
                  onPrevious={cells.onPrevious}
                />
              )}
            </section>
          </div>
          <div id="evidence" className="scroll-mt-4">
            <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-2">
              {/* Board ScriptDetail (table-rows L1-5): one paragraph, then the script's facts. */}
              <DetailSection title="How to read this page">
                <p className="text-body max-w-[640px]">
                  <ScriptHistoryNotes family={family} chart={chart} lineage={lineage} inline />{' '}
                  <ScriptSummarySentence family={family} lineage={lineage} inline />{' '}
                  <ScriptCustodyMechanism
                    family={family}
                    lineage={lineage}
                    binaries={binaries}
                    inline
                  />
                </p>
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
