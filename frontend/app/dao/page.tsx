'use client';

import { useState, useMemo } from 'react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { PageHeader } from '@/components/ui/page-header';
import { StatCard, FilterButtonGroup } from '@/components/ui/chart-card';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, DaoDeposit, ScriptLookupResponse } from '@/lib/api';
import { formatTimeAgo, formatCkbAmount, formatCkbValue, formatNumber } from '@/lib/utils';

function ScriptLabel({
  codeHash,
  scriptLookup,
}: {
  codeHash: string | null | undefined;
  scriptLookup?: ScriptLookupResponse;
}) {
  if (!codeHash || !scriptLookup) return null;
  const info = scriptLookup[codeHash];
  if (!info) return null;

  return (
    <Link
      href={`/scripts/${encodeURIComponent(info.name)}`}
      className="bg-info/10 text-info inline-flex items-center rounded px-2 py-0.5 text-xs hover:opacity-80"
    >
      {info.name}
    </Link>
  );
}

function InteractivePieChart({
  data,
  size = 120,
  hoveredIndex,
  onHover,
}: {
  data: { label: string; value: number; color: string; percent: string }[];
  size?: number;
  hoveredIndex: number | null;
  onHover: (index: number | null) => void;
}) {
  if (data.length === 0) return null;
  const total = data.reduce((acc, d) => acc + d.value, 0);
  let currentAngle = 0;
  const cx = size / 2;
  const cy = size / 2;
  const r = size * 0.38;

  const paths = data.map((d, idx) => {
    const angle = (d.value / total) * 360;
    const startAngle = currentAngle;
    const endAngle = currentAngle + angle;
    currentAngle = endAngle;

    const startRad = (startAngle - 90) * (Math.PI / 180);
    const endRad = (endAngle - 90) * (Math.PI / 180);
    const largeArc = angle > 180 ? 1 : 0;

    const isHovered = hoveredIndex === idx;
    const scale = isHovered ? 1.08 : 1;
    const currentR = r * scale;

    const x1 = cx + currentR * Math.cos(startRad);
    const y1 = cy + currentR * Math.sin(startRad);
    const x2 = cx + currentR * Math.cos(endRad);
    const y2 = cy + currentR * Math.sin(endRad);

    return {
      d: `M ${cx} ${cy} L ${x1} ${y1} A ${currentR} ${currentR} 0 ${largeArc} 1 ${x2} ${y2} Z`,
      color: d.color,
      percent: d.percent,
      isHovered,
    };
  });

  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="cursor-pointer">
      {paths.map((p, i) => (
        <path
          key={i}
          d={p.d}
          fill={p.color}
          opacity={hoveredIndex === null || p.isHovered ? 1 : 0.5}
          onMouseEnter={() => onHover(i)}
          onMouseLeave={() => onHover(null)}
          className="transition-opacity duration-150"
        />
      ))}
      {hoveredIndex !== null && (
        <text
          x={cx}
          y={cy}
          textAnchor="middle"
          dominantBaseline="middle"
          className="pointer-events-none fill-white font-mono text-lg font-bold"
        >
          {paths[hoveredIndex].percent}%
        </text>
      )}
    </svg>
  );
}

export default function DaoPage() {
  const depositsPagination = useCursorPagination();
  const [status, setStatus] = useState<number>(0);
  const [secondaryHover, setSecondaryHover] = useState<number | null>(null);
  const [compensationHover, setCompensationHover] = useState<number | null>(null);

  const { data: stats } = useQuery({
    queryKey: ['dao-statistics'],
    queryFn: () => api.getDaoStatistics(),
  });

  const { data: deposits, isLoading } = useQuery({
    queryKey: ['dao-deposits', depositsPagination.cursor, status],
    queryFn: () => api.getDaoDeposits({ limit: 20, status, cursor: depositsPagination.cursor }),
    placeholderData: keepPreviousData,
  });

  const codeHashes = useMemo(() => {
    if (!deposits?.data) return [];
    const hashes = new Set<string>();
    for (const d of deposits.data) {
      if (d.lockCodeHash) hashes.add(d.lockCodeHash);
    }
    return Array.from(hashes);
  }, [deposits]);

  const { data: scriptLookup } = useQuery({
    queryKey: ['scriptLookup', codeHashes],
    queryFn: () => api.lookupScripts(codeHashes),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });

  const getSecondaryIssuanceData = () => {
    if (!stats) return [];
    const mining = parseFloat(stats.miningRewardCkb) || 0;
    const deposit = parseFloat(stats.depositCompensationCkb) || 0;
    const burnt = parseFloat(stats.burntCkb) || 0;
    const total = mining + deposit + burnt;
    if (total === 0) return [];
    return [
      {
        label: 'Mining Reward',
        value: mining,
        color: '#8ce00a',
        percent: ((mining / total) * 100).toFixed(1),
      },
      {
        label: 'Deposit Compensation',
        value: deposit,
        color: '#ffb900',
        percent: ((deposit / total) * 100).toFixed(1),
      },
      {
        label: 'Burnt',
        value: burnt,
        color: '#6b6860',
        percent: ((burnt / total) * 100).toFixed(1),
      },
    ];
  };

  const getCompensationData = () => {
    if (!stats) return [];
    const claimed = parseFloat(stats.totalCompensationPaidCkb) || 0;
    const unclaimed = parseFloat(stats.unclaimedCompensationCkb) || 0;
    const total = claimed + unclaimed;
    if (total === 0) return [];
    return [
      {
        label: 'Claimed',
        value: claimed,
        color: '#8ce00a',
        percent: ((claimed / total) * 100).toFixed(1),
      },
      {
        label: 'Unclaimed',
        value: unclaimed,
        color: '#ffb900',
        percent: ((unclaimed / total) * 100).toFixed(1),
      },
    ];
  };

  const filterOptions = [
    { label: 'Active Deposits', value: 0 },
    { label: 'Withdraw Request', value: 1 },
    { label: 'Withdrawn', value: 2 },
  ];
  const selectedFilterLabel =
    filterOptions.find((option) => option.value === status)?.label ?? 'Active Deposits';

  const renderReferenceCell = (deposit: DaoDeposit) => {
    const depositCellHref = `/cell/${deposit.txHash}-${deposit.outputIndex}`;
    const depositCellLabel = `${deposit.txHash}:${deposit.outputIndex}`;

    if (
      deposit.status === 'withdrawing' &&
      deposit.withdrawRequestTxHash &&
      deposit.withdrawRequestOutputIndex !== null
    ) {
      const requestCellHref = `/cell/${deposit.withdrawRequestTxHash}-${deposit.withdrawRequestOutputIndex}`;
      const requestCellLabel = `${deposit.withdrawRequestTxHash}:${deposit.withdrawRequestOutputIndex}`;
      return (
        <div className="space-y-1">
          <Link href={requestCellHref} className="text-emphasis hover:underline">
            <Hash hash={requestCellLabel} />
          </Link>
          <div className="text-text-dim font-mono text-xs">
            Deposit cell:{' '}
            <Link href={depositCellHref} className="hover:text-text">
              <Hash hash={depositCellLabel} />
            </Link>
          </div>
        </div>
      );
    }

    if (
      deposit.status === 'withdrawn' &&
      deposit.withdrawTxHash &&
      deposit.withdrawToOutputIndex !== null
    ) {
      const withdrawToCellHref = `/cell/${deposit.withdrawTxHash}-${deposit.withdrawToOutputIndex}`;
      const withdrawToCellLabel = `${deposit.withdrawTxHash}:${deposit.withdrawToOutputIndex}`;
      return (
        <div className="space-y-1">
          <Link href={withdrawToCellHref} className="text-emphasis hover:underline">
            <Hash hash={withdrawToCellLabel} />
          </Link>
          <div className="text-text-dim font-mono text-xs">
            Deposit cell:{' '}
            <Link href={depositCellHref} className="hover:text-text">
              <Hash hash={depositCellLabel} />
            </Link>
          </div>
        </div>
      );
    }

    if (deposit.status === 'withdrawing' && deposit.withdrawRequestTxHash) {
      return (
        <div className="space-y-1">
          <Link
            href={`/tx/${deposit.withdrawRequestTxHash}`}
            className="text-emphasis hover:underline"
          >
            <Hash hash={deposit.withdrawRequestTxHash} />
          </Link>
          <div className="text-text-dim font-mono text-xs">
            Deposit cell:{' '}
            <Link href={depositCellHref} className="hover:text-text">
              <Hash hash={depositCellLabel} />
            </Link>
          </div>
        </div>
      );
    }

    if (deposit.status === 'withdrawn' && deposit.withdrawTxHash) {
      return (
        <div className="space-y-1">
          <Link href={`/tx/${deposit.withdrawTxHash}`} className="text-emphasis hover:underline">
            <Hash hash={deposit.withdrawTxHash} />
          </Link>
          <div className="text-text-dim font-mono text-xs">
            Deposit cell:{' '}
            <Link href={depositCellHref} className="hover:text-text">
              <Hash hash={depositCellLabel} />
            </Link>
          </div>
        </div>
      );
    }

    return (
      <Link href={depositCellHref} className="text-emphasis hover:underline">
        <Hash hash={depositCellLabel} />
      </Link>
    );
  };

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <PageHeader
          title="Nervos DAO"
          subtitle="Deposit CKB to earn compensation from secondary issuance"
          actions={
            <Link
              href="/charts"
              className="border-base-border bg-base-elevated text-text hover:bg-base-elevated/80 hover:text-text-bright rounded border px-4 py-2 font-mono text-sm transition-colors"
            >
              View Charts
            </Link>
          }
        />

        <TerminalPanel className="mb-4" glow>
          <TerminalPanelContent>
            <div className="mb-4 text-center">
              <div className="text-text-dim font-mono text-xs uppercase tracking-wider">
                Total Deposit
              </div>
              <div className="text-emphasis font-display mt-2 text-4xl font-bold tabular-nums">
                {stats
                  ? (() => {
                      const f = formatCkbValue(stats.totalDepositedCkb);
                      return (
                        <>
                          {f.integer}
                          <span className="text-emphasis/50 text-[0.85em]">.{f.decimal}</span>
                          <span className="text-text-dim ml-2 text-[0.85em]">CKB</span>
                        </>
                      );
                    })()
                  : '...'}
              </div>
            </div>
            <div className="border-base-border grid gap-6 border-t pt-6 md:grid-cols-3">
              <StatCard
                label="Depositors"
                value={stats ? formatNumber(stats.totalDepositors) : '...'}
              />
              <StatCard label="Avg Deposit Time" value={stats?.averageDepositDays || '...'} />
              <StatCard
                label="Estimated APC"
                value={stats?.estimatedApc ? `${stats.estimatedApc}%` : '...'}
                valueClassName="font-display"
              />
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel className="mb-4">
          <TerminalPanelContent>
            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <div className="text-text-dim mb-4 font-mono text-xs uppercase tracking-wider">
                  Secondary Issuance
                </div>
                <div className="flex items-center gap-6">
                  <InteractivePieChart
                    data={getSecondaryIssuanceData()}
                    size={120}
                    hoveredIndex={secondaryHover}
                    onHover={setSecondaryHover}
                  />
                  <div className="space-y-3">
                    {getSecondaryIssuanceData().map((item, idx) => (
                      <div
                        key={item.label}
                        className="flex cursor-pointer items-center gap-2"
                        onMouseEnter={() => setSecondaryHover(idx)}
                        onMouseLeave={() => setSecondaryHover(null)}
                      >
                        <span
                          className="h-2.5 w-2.5 shrink-0 rounded-full transition-transform duration-150"
                          style={{
                            backgroundColor: item.color,
                            transform: secondaryHover === idx ? 'scale(1.3)' : 'scale(1)',
                          }}
                        />
                        <div className="min-w-0">
                          <div
                            className={`text-xs transition-colors duration-150 ${secondaryHover === idx ? 'text-text-bright' : 'text-text-dim'}`}
                          >
                            {item.label}
                          </div>
                          <div
                            className={`truncate font-mono text-sm font-medium tabular-nums transition-colors duration-150 ${secondaryHover === idx ? 'text-text-bright' : 'text-text'}`}
                          >
                            {(() => {
                              const f = formatCkbValue(item.value);
                              return (
                                <>
                                  {f.integer}
                                  <span
                                    className={`text-[0.85em] ${secondaryHover === idx ? 'text-text' : 'text-text-dim'}`}
                                  >
                                    .{f.decimal}
                                  </span>
                                </>
                              );
                            })()}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>

              <div className="border-base-border border-l pl-8">
                <div className="text-text-dim mb-4 font-mono text-xs uppercase tracking-wider">
                  Compensation
                </div>
                <div className="flex items-center gap-6">
                  <InteractivePieChart
                    data={getCompensationData()}
                    size={120}
                    hoveredIndex={compensationHover}
                    onHover={setCompensationHover}
                  />
                  <div className="space-y-3">
                    {getCompensationData().map((item, idx) => (
                      <div
                        key={item.label}
                        className="flex cursor-pointer items-center gap-2"
                        onMouseEnter={() => setCompensationHover(idx)}
                        onMouseLeave={() => setCompensationHover(null)}
                      >
                        <span
                          className="h-2.5 w-2.5 shrink-0 rounded-full transition-transform duration-150"
                          style={{
                            backgroundColor: item.color,
                            transform: compensationHover === idx ? 'scale(1.3)' : 'scale(1)',
                          }}
                        />
                        <div className="min-w-0">
                          <div
                            className={`text-xs transition-colors duration-150 ${compensationHover === idx ? 'text-text-bright' : 'text-text-dim'}`}
                          >
                            {item.label}
                          </div>
                          <div
                            className={`truncate font-mono text-sm font-medium tabular-nums transition-colors duration-150 ${compensationHover === idx ? 'text-text-bright' : 'text-text'}`}
                          >
                            {(() => {
                              const f = formatCkbValue(item.value);
                              return (
                                <>
                                  {f.integer}
                                  <span
                                    className={`text-[0.85em] ${compensationHover === idx ? 'text-text' : 'text-text-dim'}`}
                                  >
                                    .{f.decimal}
                                  </span>
                                </>
                              );
                            })()}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel>
          <TerminalPanelHeader
            indicator="active"
            actions={
              <FilterButtonGroup
                options={filterOptions}
                selected={status}
                onChange={(v) => {
                  setStatus(v as number);
                  depositsPagination.reset();
                }}
              />
            }
          >
            {selectedFilterLabel}
          </TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {isLoading ? (
              <div className="text-text-dim py-8 text-center">Loading...</div>
            ) : deposits?.data?.length ? (
              <>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="border-base-border text-text-dim border-b text-left font-mono text-xs uppercase">
                        <th className="px-4 py-3">Reference</th>
                        <th className="px-4 py-3">Address</th>
                        <th className="px-4 py-3 text-right">Amount</th>
                        <th className="px-4 py-3 text-right">Time</th>
                      </tr>
                    </thead>
                    <tbody>
                      {deposits.data.map((deposit: DaoDeposit) => (
                        <tr
                          key={`${deposit.txHash}-${deposit.outputIndex}`}
                          className="hover:bg-base-elevated/50 border-base-border/50 border-b transition-colors"
                        >
                          <td className="px-4 py-3">{renderReferenceCell(deposit)}</td>
                          <td className="px-4 py-3">
                            <div className="flex items-center gap-2">
                              {deposit.address ? (
                                <Address address={deposit.address} />
                              ) : (
                                <Link href={`/address/${deposit.lockScriptHash}`}>
                                  <Hash
                                    hash={deposit.lockScriptHash}
                                    className="hover:text-emphasis text-text"
                                  />
                                </Link>
                              )}
                              <ScriptLabel
                                codeHash={deposit.lockCodeHash}
                                scriptLookup={scriptLookup}
                              />
                            </div>
                          </td>
                          <td className="text-text-bright px-4 py-3 text-right font-mono tabular-nums">
                            {(() => {
                              const f = formatCkbAmount(deposit.capacity);
                              return (
                                <>
                                  {f.integer}
                                  <span className="text-text-dim text-[0.85em]">.{f.decimal}</span>
                                  <span className="text-text-dim ml-1 text-[0.85em]">CKB</span>
                                </>
                              );
                            })()}
                          </td>
                          <td className="text-text-dim px-4 py-3 text-right text-sm">
                            {formatTimeAgo(deposit.depositTimestamp)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="border-base-border border-t px-4 py-3">
                  <CursorPagination
                    total={deposits.total ?? undefined}
                    totalLabel="deposits"
                    pageSize={20}
                    page={depositsPagination.page}
                    currentCount={deposits.data.length}
                    hasMore={deposits.hasMore}
                    hasPrevious={depositsPagination.hasPrevious}
                    onNext={() => depositsPagination.goToNext(deposits.nextCursor)}
                    onPrevious={depositsPagination.goToPrevious}
                  />
                </div>
              </>
            ) : (
              <div className="text-text-dim py-8 text-center">No deposits found</div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
