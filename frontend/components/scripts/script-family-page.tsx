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
import { ScriptCustodyEvidence } from '@/components/scripts/script-code-evidence';
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
  ScriptUseInTransaction,
  blockLink,
} from '@/components/scripts/script-family-board';
import type { Cell, ScriptFamilyDetail, StackedAreaChartResponse } from '@/lib/api';
import type { CapacityRangeKey } from '@/lib/capacity-range';
import { formatCapacity, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
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

/** Tier 2's "Registered references" table: each reference, its current code and its holder. */
const REFERENCE_COLUMNS: ListColumn<ReferenceRow>[] = [
  {
    key: 'reference',
    label: 'Reference',
    minWidth: 250,
    width: '35%',
    render: (row) => (
      <span className="text-text-bright break-all">
        {row.hashType} · {row.referenceHash}
      </span>
    ),
  },
  {
    key: 'code',
    label: 'Current code version',
    minWidth: 210,
    width: '30%',
    render: (row) => (
      <span className="text-text-dim break-all">
        {row.currentVersionHash ?? 'Retired · no current code cell'}
      </span>
    ),
  },
  {
    key: 'custody',
    label: 'Code cell holder',
    minWidth: 235,
    width: '35%',
    render: (row) =>
      row.custody ? (
        <ScriptCustodyEvidence custody={row.custody} />
      ) : (
        <span className="text-text-dim">Evidence unavailable</span>
      ),
  },
];

/** A tier-1 section (hierarchy R6): h2 at section size, 18 px above its evidence. */
function Section({
  title,
  className,
  children,
}: {
  title: string;
  className: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className={`min-w-0 ${className}`}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">{title}</h2>
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
  const deploymentOf = new Map(
    orderedVersions(family).flatMap((version) =>
      version.deployments.map(
        (deployment) =>
          [`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`, version] as const
      )
    )
  );
  const rows = orderedVersions(family).map((version) => ({
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
  /** Board L7's 640 table: five columns, the version hash the code cell's second line. */
  const narrowColumns: ListColumn<Row>[] = [
    { key: 'version', label: 'v', minWidth: 64, render: versionCell },
    {
      key: 'deployed',
      label: 'Deployed',
      minWidth: 96,
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
      className: 'text-text-dim',
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
              <ScriptCustodyFigure family={family} lineage={lineage} holders={holders} />
            </Section>
            <Section title="Code versions" className="mt-[72px]">
              <ListTable
                label="Code versions"
                rows={rows}
                rowKey={(row) => row.version.versionHash}
                columns={[
                  {
                    key: 'version',
                    label: 'Version',
                    minWidth: 70,
                    width: '6%',
                    render: versionCell,
                  },
                  {
                    key: 'deployed',
                    label: 'Deployed (UTC+8)',
                    minWidth: 150,
                    width: '15%',
                    render: (row) =>
                      row.version.deployments.length ? (
                        row.version.deployments.map((deployment) => (
                          <span key={outpointKey(deployment)} className="block">
                            {formatUtc8DateTime(deployment.deployedAt).slice(0, 16)}
                            {deployment.createdAtBlock !== undefined && (
                              <span className="text-meta block">
                                {blockLink(deployment.createdAtBlock)}
                              </span>
                            )}
                          </span>
                        ))
                      ) : (
                        <span className="text-text-dim">No deployment observed</span>
                      ),
                  },
                  {
                    key: 'binary',
                    label: 'Binary',
                    minWidth: 90,
                    width: '8%',
                    numeric: true,
                    render: (row) =>
                      row.version.deployments.map((deployment) => (
                        <span
                          key={outpointKey(deployment)}
                          data-testid="code-binary-size"
                          className="block"
                        >
                          {binarySize(deployment)}
                        </span>
                      )),
                  },
                  {
                    key: 'hash',
                    label: 'Data hash',
                    minWidth: 220,
                    width: '24%',
                    render: (row) => (
                      <span className="text-text-dim text-meta [overflow-wrap:anywhere]">
                        {row.version.versionHash}
                      </span>
                    ),
                  },
                  {
                    key: 'cell',
                    label: 'Code cell',
                    minWidth: 190,
                    width: '17%',
                    render: (row) =>
                      row.version.deployments.map((deployment) => {
                        const state = codeCells.get(outpointKey(deployment));
                        return (
                          <span key={outpointKey(deployment)} className="block">
                            {outpointLink(deployment)}
                            <span className="text-text-dim text-meta block">
                              {state?.cell && (
                                <>
                                  <ExactCkb
                                    shannons={BigInt(state.cell.capacity)}
                                    signed={false}
                                    tone="plain"
                                  />{' '}
                                  ·{' '}
                                </>
                              )}
                              {deploymentStatus(deployment)}
                            </span>
                          </span>
                        );
                      }),
                  },
                  {
                    key: 'signed',
                    label: 'Signed by',
                    minWidth: 190,
                    width: '18%',
                    render: (row) =>
                      row.version.deployments.map((deployment) => {
                        const { who, note, marked } = signer(deployment);
                        return marked ? (
                          <span key={outpointKey(deployment)} className="block text-[#a0a8be]">
                            {who}
                            <span className="text-text-dim text-meta block">{note}</span>
                          </span>
                        ) : (
                          <span key={outpointKey(deployment)} className="text-text-dim block">
                            {who}
                          </span>
                        );
                      }),
                  },
                  {
                    key: 'usage',
                    label: 'Live cells using it',
                    minWidth: 110,
                    width: '12%',
                    numeric: true,
                    render: (row) => versionUsage(row.version.versionHash),
                  },
                ]}
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
                  columns={[
                    {
                      key: 'cell',
                      label: 'Cell',
                      minWidth: 160,
                      width: '15%',
                      render: (cell) => (
                        <Link
                          href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                          className="text-text hover:text-jade hover:underline"
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
            <DetailTier2>
              <DetailSection title="How to read this page">
                <div className="space-y-3">
                  <ScriptSummarySentence family={family} lineage={lineage} />
                  <ScriptHistoryNotes family={family} chart={chart} lineage={lineage} />
                  <p className="text-body max-w-[640px]">
                    <ScriptCellsSummary family={family} />
                  </p>
                </div>
              </DetailSection>
              <DetailSection title="How its code can change">
                <div className="space-y-3">
                  <ScriptCustodyMechanism family={family} lineage={lineage} binaries={binaries} />
                </div>
              </DetailSection>
              <DetailSection title="Use it in a transaction">
                <ScriptUseInTransaction family={family} />
              </DetailSection>
              <DetailSection title="Registered references">
                <ListTable
                  label="Registered references"
                  rows={family.references}
                  rowKey={(row) => `${row.hashType}:${row.referenceHash}`}
                  columns={REFERENCE_COLUMNS}
                  mobileRow={(row) => (
                    <div className="text-small space-y-2">
                      <div className="break-all">
                        {row.hashType} · {row.referenceHash}
                      </div>
                      <div className="break-all">
                        Current code: {row.currentVersionHash ?? 'retired'}
                      </div>
                      {row.custody ? (
                        <ScriptCustodyEvidence custody={row.custody} />
                      ) : (
                        'Evidence unavailable'
                      )}
                    </div>
                  )}
                />
              </DetailSection>
            </DetailTier2>
          </div>
        </div>
      </main>
    </div>
  );
}
