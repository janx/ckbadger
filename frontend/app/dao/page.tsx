'use client';

import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { Header } from '@/components/layout/header';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { StatCard, FilterButtonGroup } from '@/components/ui/chart-card';
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
      className="inline-flex items-center rounded bg-blue-900/50 px-2 py-0.5 text-xs text-blue-400 hover:opacity-80"
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
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  const [cursorHistory, setCursorHistory] = useState<string[]>([]);
  const [status, setStatus] = useState<number | undefined>(undefined);
  const [secondaryHover, setSecondaryHover] = useState<number | null>(null);
  const [compensationHover, setCompensationHover] = useState<number | null>(null);

  const { data: stats } = useQuery({
    queryKey: ['dao-statistics'],
    queryFn: () => api.getDaoStatistics(),
  });

  const { data: deposits, isLoading } = useQuery({
    queryKey: ['dao-deposits', cursor, status],
    queryFn: () => api.getDaoDeposits({ limit: 20, status, cursor }),
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

  const handleNextPage = () => {
    if (deposits?.nextCursor) {
      setCursorHistory((prev) => [...prev, cursor || '']);
      setCursor(deposits.nextCursor);
    }
  };

  const handlePrevPage = () => {
    if (cursorHistory.length > 0) {
      const prev = cursorHistory[cursorHistory.length - 1];
      setCursorHistory((h) => h.slice(0, -1));
      setCursor(prev || undefined);
    }
  };

  const resetPagination = () => {
    setCursor(undefined);
    setCursorHistory([]);
  };

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
        color: '#00ff41',
        percent: ((mining / total) * 100).toFixed(1),
      },
      {
        label: 'Deposit Compensation',
        value: deposit,
        color: '#ffb000',
        percent: ((deposit / total) * 100).toFixed(1),
      },
      {
        label: 'Burnt',
        value: burnt,
        color: '#3d4a5c',
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
        color: '#00ff41',
        percent: ((claimed / total) * 100).toFixed(1),
      },
      {
        label: 'Unclaimed',
        value: unclaimed,
        color: '#ffb000',
        percent: ((unclaimed / total) * 100).toFixed(1),
      },
    ];
  };

  const getStatusBadge = (depositStatus: string) => {
    switch (depositStatus) {
      case 'deposited':
        return <Badge variant="green">Active</Badge>;
      case 'withdrawing':
        return <Badge variant="amber">Withdrawing</Badge>;
      case 'withdrawn':
        return <Badge variant="gray">Withdrawn</Badge>;
      default:
        return null;
    }
  };

  const filterOptions = [
    { label: 'All', value: undefined },
    { label: 'Active', value: 0 },
    { label: 'Withdrawing', value: 1 },
    { label: 'Withdrawn', value: 2 },
  ];

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Nervos DAO"
          subtitle="Deposit CKB to earn compensation from secondary issuance"
          actions={
            <Link
              href="/charts"
              className="rounded border border-slate-700 bg-slate-800 px-4 py-2 font-mono text-sm text-slate-300 transition-colors hover:bg-slate-700 hover:text-white"
            >
              View Charts
            </Link>
          }
        />

        <TerminalPanel className="mb-6" glow>
          <TerminalPanelContent>
            <div className="mb-6 text-center">
              <div className="font-mono text-xs uppercase tracking-wider text-slate-500">
                Total Deposit
              </div>
              <div className="text-terminal-green mt-2 font-mono text-4xl font-bold tabular-nums">
                {stats
                  ? (() => {
                      const f = formatCkbValue(stats.totalDepositedCkb);
                      return (
                        <>
                          {f.integer}
                          <span className="text-terminal-green/50 text-[0.85em]">.{f.decimal}</span>
                          <span className="ml-2 text-[0.85em] text-slate-500">CKB</span>
                        </>
                      );
                    })()
                  : '...'}
              </div>
            </div>
            <div className="grid gap-6 border-t border-slate-800 pt-6 md:grid-cols-3">
              <StatCard
                label="Depositors"
                value={stats ? formatNumber(stats.totalDepositors) : '...'}
              />
              <StatCard label="Avg Deposit Time" value={stats?.averageDepositDays || '...'} />
              <StatCard
                label="Estimated APC"
                value={stats?.estimatedApc ? `${stats.estimatedApc}%` : '...'}
              />
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel className="mb-6">
          <TerminalPanelContent>
            <div className="grid gap-8 md:grid-cols-2">
              <div>
                <div className="mb-4 font-mono text-xs uppercase tracking-wider text-slate-500">
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
                            className={`text-xs transition-colors duration-150 ${secondaryHover === idx ? 'text-white' : 'text-slate-500'}`}
                          >
                            {item.label}
                          </div>
                          <div
                            className={`truncate font-mono text-sm font-medium tabular-nums transition-colors duration-150 ${secondaryHover === idx ? 'text-white' : 'text-slate-300'}`}
                          >
                            {(() => {
                              const f = formatCkbValue(item.value);
                              return (
                                <>
                                  {f.integer}
                                  <span
                                    className={`text-[0.85em] ${secondaryHover === idx ? 'text-slate-300' : 'text-slate-600'}`}
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

              <div className="border-l border-slate-800 pl-8">
                <div className="mb-4 font-mono text-xs uppercase tracking-wider text-slate-500">
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
                            className={`text-xs transition-colors duration-150 ${compensationHover === idx ? 'text-white' : 'text-slate-500'}`}
                          >
                            {item.label}
                          </div>
                          <div
                            className={`truncate font-mono text-sm font-medium tabular-nums transition-colors duration-150 ${compensationHover === idx ? 'text-white' : 'text-slate-300'}`}
                          >
                            {(() => {
                              const f = formatCkbValue(item.value);
                              return (
                                <>
                                  {f.integer}
                                  <span
                                    className={`text-[0.85em] ${compensationHover === idx ? 'text-slate-300' : 'text-slate-600'}`}
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
                  setStatus(v as number | undefined);
                  resetPagination();
                }}
              />
            }
          >
            Deposits
          </TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {isLoading ? (
              <div className="py-8 text-center text-slate-500">Loading...</div>
            ) : deposits?.data?.length ? (
              <>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="border-b border-slate-800 text-left font-mono text-xs uppercase text-slate-500">
                        <th className="px-4 py-3">Cell</th>
                        <th className="px-4 py-3">Address</th>
                        <th className="px-4 py-3 text-right">Amount</th>
                        <th className="px-4 py-3">Status</th>
                        <th className="px-4 py-3 text-right">Time</th>
                      </tr>
                    </thead>
                    <tbody>
                      {deposits.data.map((deposit: DaoDeposit) => (
                        <tr
                          key={`${deposit.txHash}-${deposit.outputIndex}`}
                          className="hover:bg-slate-850/50 border-b border-slate-800/50 transition-colors"
                        >
                          <td className="px-4 py-3">
                            <Link
                              href={`/cell/${deposit.txHash}-${deposit.outputIndex}`}
                              className="text-terminal-green hover:underline"
                            >
                              <Hash hash={`${deposit.txHash}:${deposit.outputIndex}`} />
                            </Link>
                          </td>
                          <td className="px-4 py-3">
                            <div className="flex items-center gap-2">
                              {deposit.address ? (
                                <Address address={deposit.address} />
                              ) : (
                                <Link href={`/address/${deposit.lockScriptHash}`}>
                                  <Hash
                                    hash={deposit.lockScriptHash}
                                    className="hover:text-terminal-green text-slate-400"
                                  />
                                </Link>
                              )}
                              <ScriptLabel
                                codeHash={deposit.lockCodeHash}
                                scriptLookup={scriptLookup}
                              />
                            </div>
                          </td>
                          <td className="px-4 py-3 text-right font-mono tabular-nums text-white">
                            {(() => {
                              const f = formatCkbAmount(deposit.capacity);
                              return (
                                <>
                                  {f.integer}
                                  <span className="text-[0.85em] text-slate-500">.{f.decimal}</span>
                                  <span className="ml-1 text-[0.85em] text-slate-600">CKB</span>
                                </>
                              );
                            })()}
                          </td>
                          <td className="px-4 py-3">{getStatusBadge(deposit.status)}</td>
                          <td className="px-4 py-3 text-right text-sm text-slate-500">
                            {formatTimeAgo(deposit.depositTimestamp)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="flex items-center justify-between border-t border-slate-800 px-4 py-3">
                  <span className="font-mono text-sm text-slate-500">
                    Total: {deposits.total.toLocaleString()} deposits
                  </span>
                  <div className="flex gap-2">
                    <button
                      onClick={handlePrevPage}
                      disabled={cursorHistory.length === 0}
                      className="rounded border border-slate-700 bg-slate-800 px-3 py-1.5 font-mono text-sm text-slate-300 transition-colors hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Previous
                    </button>
                    <button
                      onClick={handleNextPage}
                      disabled={!deposits.hasMore}
                      className="rounded border border-slate-700 bg-slate-800 px-3 py-1.5 font-mono text-sm text-slate-300 transition-colors hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Next
                    </button>
                  </div>
                </div>
              </>
            ) : (
              <div className="py-8 text-center text-slate-500">No deposits found</div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
