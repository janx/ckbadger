'use client';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { CellBar } from '@/components/ui/cell-bar';
import { CapacityRangeSelector } from '@/components/ui/capacity-range-selector';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import { ExactCkb } from '@/components/activity-event-row';
import { ScriptCustodyEvidence } from '@/components/scripts/script-code-evidence';
import {
  ScriptCellsSummary,
  ScriptCustodyFigure,
  ScriptFactsRail,
  ScriptHeading,
  ScriptHistory,
  ScriptSummarySentence,
  ScriptUseInTransaction,
  blockLink,
} from '@/components/scripts/script-family-board';
import type { Cell, ScriptFamilyDetail, StackedAreaChartResponse } from '@/lib/api';
import type { CapacityRangeKey } from '@/lib/capacity-range';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
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
  const typeReference = lineage?.[0]?.referenceHash;
  const roleCrumb =
    family.scriptKind === 'lock' ? 'Locks' : family.scriptKind === 'type' ? 'Types' : null;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <nav aria-label="Breadcrumb" className="text-text-dim mb-2 text-xs">
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
        <DetailLayout
          title={family.name}
          status={
            <>
              <ScriptHeading family={family} />
              {deprecated && <span className="text-text-dim text-xs">· Deprecated</span>}
            </>
          }
          identity={
            <span className="text-text text-[13px]">
              {family.description}
              {family.website && (
                <>
                  {family.description ? ' · ' : ''}
                  <a href={family.website} className="text-[#6aa9c4] hover:underline">
                    {family.website.replace(/^https?:\/\//, '').replace(/\/$/, '')} ↗
                  </a>
                </>
              )}
            </span>
          }
          tier2={
            <DetailSection title="Registered references">
              <ListTable
                label="Registered references"
                rows={family.references}
                rowKey={(row) => `${row.hashType}:${row.referenceHash}`}
                columns={[
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
                ]}
                mobileRow={(row) => (
                  <div className="space-y-2 text-xs">
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
          }
        >
          <ScriptSummarySentence family={family} lineage={lineage} />
          <div className="grid items-start gap-12 lg:grid-cols-[minmax(0,1fr)_300px]">
            <div className="min-w-0 space-y-11">
              <DetailSection
                title="History"
                actions={<CapacityRangeSelector value={capacityRange} onChange={onCapacityRange} />}
              >
                {chartLoading ? (
                  <p className="text-text-dim py-6 text-xs">Loading capacity history…</p>
                ) : chartError ? (
                  <p role="alert" className="text-negative text-xs">
                    Capacity history unavailable: {chartError.message}
                  </p>
                ) : chart?.data.length ? (
                  <ScriptHistory
                    family={family}
                    chart={chart}
                    lineage={lineage}
                    numbers={numbers}
                    holders={holders}
                  />
                ) : (
                  <p className="text-text-dim py-6 text-xs">No capacity history yet</p>
                )}
              </DetailSection>
              <DetailSection title="Who can change its code">
                <ScriptCustodyFigure family={family} lineage={lineage} holders={holders} />
              </DetailSection>
            </div>
            <ScriptFactsRail family={family} lineage={lineage} />
          </div>
          <DetailSection
            title="Code versions"
            actions={
              <span className="text-text-dim text-xs">
                {typeReference
                  ? `one type id, ${shortHex(typeReference, 8, 6)}, ${rows.length} binaries`
                  : `${rows.length} binaries`}
              </span>
            }
          >
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
                  render: (row) => (
                    <span className="text-text-bright">
                      v{row.number}
                      {row.current && <span className="text-jade block text-[11px]">current</span>}
                    </span>
                  ),
                },
                {
                  key: 'deployed',
                  label: 'Deployed (UTC+8)',
                  minWidth: 150,
                  width: '15%',
                  render: (row) =>
                    row.version.deployments.length ? (
                      row.version.deployments.map((deployment) => (
                        <span
                          key={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
                          className="block"
                        >
                          {formatUtc8DateTime(deployment.deployedAt).slice(0, 16)}
                          {deployment.createdAtBlock !== undefined && (
                            <span className="block text-[11px]">
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
                    row.version.deployments.map((deployment) => {
                      const key = `${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`;
                      const state = codeCells.get(key);
                      return (
                        <span key={key} data-testid="code-binary-size" className="block">
                          {state?.cell ? (
                            `${state.cell.dataSize.toLocaleString('en-US')} B`
                          ) : state?.error ? (
                            <span role="alert" title={state.error.message} className="text-rouge">
                              unavailable
                            </span>
                          ) : (
                            <span className="text-text-dim">…</span>
                          )}
                        </span>
                      );
                    }),
                },
                {
                  key: 'hash',
                  label: 'Data hash',
                  minWidth: 220,
                  width: '24%',
                  render: (row) => (
                    <span className="text-text-dim text-[11px] [overflow-wrap:anywhere]">
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
                      const key = `${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`;
                      const state = codeCells.get(key);
                      const successor = deployment.consumedByTx
                        ? [...deploymentOf].find(([outpoint]) =>
                            outpoint.startsWith(`${deployment.consumedByTx}:`)
                          )?.[1]
                        : undefined;
                      return (
                        <span key={key} className="block">
                          <Link
                            href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                            className="text-[#6aa9c4] hover:underline"
                          >
                            {shortHex(deployment.codeCellTxHash, 8, 6)}:
                            {deployment.codeCellOutputIndex}
                          </Link>
                          <span className="text-text-dim block text-[11px]">
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
                            {deployment.status === 'live'
                              ? 'live'
                              : successor
                                ? `consumed by v${numbers.get(successor.versionHash)}`
                                : deployment.consumedByTx
                                  ? `consumed in ${shortHex(deployment.consumedByTx, 8, 6)}`
                                  : 'consumed'}
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
                      const marker = history.find(
                        (m) =>
                          m.event === 'deployed' &&
                          m.codeCellTxHash === deployment.codeCellTxHash &&
                          m.codeCellOutputIndex === deployment.codeCellOutputIndex
                      );
                      const signer = marker ? signerOf(marker, history) : null;
                      const key = `${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`;
                      if (!marker)
                        return (
                          <span key={key} className="text-text-dim block">
                            {deployment.custody
                              ? `held by ${holderName(deployment.custody)}`
                              : 'custody unavailable'}
                          </span>
                        );
                      return (
                        <span key={key} className="block text-[#a0a8be]">
                          {signer ? holderName(signer) : 'first deployed'}
                          <span className="text-text-dim block text-[11px]">
                            {!signer
                              ? `into ${holderName(marker.custody)}`
                              : holderName(signer) !== holderName(marker.custody)
                                ? `hands it to ${holderName(marker.custody)}`
                                : ''}
                          </span>
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
              mobileRow={(row) => (
                <div className="space-y-1 text-xs">
                  <div className="text-text-bright">
                    v{row.number}
                    {row.current ? ' · current' : ''}
                  </div>
                  {row.version.deployments.map((deployment) => (
                    <div key={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}>
                      {formatUtc8Date(deployment.deployedAt)} ·{' '}
                      <Link
                        href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
                        className="text-[#6aa9c4]"
                      >
                        {shortHex(deployment.codeCellTxHash, 8, 6)}:{deployment.codeCellOutputIndex}
                      </Link>{' '}
                      · {deployment.status ?? 'status unavailable'}
                    </div>
                  ))}
                  {!row.version.deployments.length && (
                    <div className="text-text-dim">No deployment observed</div>
                  )}
                  <div className="text-text-dim break-all text-[11px]">
                    {row.version.versionHash}
                  </div>
                  <div>Live cells using it: {versionUsage(row.version.versionHash)}</div>
                </div>
              )}
            />
          </DetailSection>
          <DetailSection title="Use it in a transaction">
            <ScriptUseInTransaction family={family} />
          </DetailSection>
          <section id="its-cells" className="min-w-0">
            <div className="mb-3 flex flex-wrap items-baseline gap-x-4 gap-y-1">
              <h2 className="text-text-bright text-[15px] font-medium">Its cells</h2>
              <ScriptCellsSummary family={family} />
            </div>
            <div className="text-text-dim mb-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px]">
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
              <p className="text-text-dim py-6 text-xs">Loading cells…</p>
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
                        className="text-[#6aa9c4] hover:underline"
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
                  <div className="min-w-0 space-y-1.5 text-xs">
                    <Link
                      href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                      className="text-[#6aa9c4]"
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
              <CursorPagination
                total={family.liveCellsCount}
                totalLabel="cells"
                pageSize={DEFAULT_PAGE_SIZE}
                page={cells.page}
                currentCount={cells.currentCount}
                hasMore={cells.hasMore}
                hasPrevious={cells.hasPrevious}
                onNext={cells.onNext}
                onPrevious={cells.onPrevious}
              />
            )}
          </section>
        </DetailLayout>
      </main>
    </div>
  );
}
