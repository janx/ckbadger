'use client';

import { useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { Block, api } from '@/lib/api';
import { ChainWave } from '@/components/chain-wave';
import { MempoolBlocks } from '@/components/mempool-blocks';
import { SparkChart } from '@/components/ui/spark-chart';

interface PipelineDashboardProps {
  initialBlocks?: Block[];
}

type HealthStatus = 'healthy' | 'watch' | 'critical';
type HealthMetricKey =
  | 'pending-queue'
  | 'queue-pressure'
  | 'mempool-size'
  | 'mempool-cycles'
  | 'min-feerate';

interface HealthMetricConfig {
  key: HealthMetricKey;
  label: string;
  value: number;
  defaultWarn: number;
  defaultCritical: number;
  valueLabel: string;
  deltaFormatter: (value: number) => string;
}

interface HealthMetric {
  key: HealthMetricKey;
  label: string;
  value: number;
  warn: number;
  critical: number;
  valueLabel: string;
  status: HealthStatus;
  progressPercent: number;
  deltaLabel: string;
  warningGap: number;
  warningDistanceLabel: string;
  warnLabel: string;
  criticalLabel: string;
}

interface FeerateEntry {
  label: string;
  shortLabel: string;
  value: number | undefined;
  tone: string;
  color: string;
}

interface ThresholdPair {
  warn: number;
  critical: number;
}

const HEALTH_HISTORY_WINDOW = 48;
const FEE_HISTORY_WINDOW = 24;
const FEE_TRAIL_POINTS = 6;

const EMPTY_HEALTH_HISTORY: Record<HealthMetricKey, number[]> = {
  'pending-queue': [],
  'queue-pressure': [],
  'mempool-size': [],
  'mempool-cycles': [],
  'min-feerate': [],
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = clamp(p, 0, 1) * (sorted.length - 1);
  const low = Math.floor(rank);
  const high = Math.ceil(rank);

  if (low === high) return sorted[low];
  const weight = rank - low;
  return sorted[low] * (1 - weight) + sorted[high] * weight;
}

function resolveAdaptiveThresholds(
  history: number[],
  defaultWarn: number,
  defaultCritical: number
): ThresholdPair {
  if (history.length < 6) {
    return { warn: defaultWarn, critical: defaultCritical };
  }

  const p75 = percentile(history, 0.75);
  const p9 = percentile(history, 0.9);
  const p98 = percentile(history, 0.98);

  const warn = Math.max(defaultWarn * 0.55, p75);
  const criticalBase = Math.max(defaultCritical * 0.55, p9 + (p98 - p9) * 0.35);
  const critical = Math.max(warn * 1.25, criticalBase);

  return { warn, critical };
}

function formatShannonsPerByte(value: number | null | undefined): string {
  if (!value || value <= 0) return 'N/A';
  return `${value.toFixed(2)} sh/B`;
}

function formatBytes(bytes: number | null | undefined): string {
  if (!bytes || bytes <= 0) return '0 B';
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(2)} MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(2)} KB`;
  return `${bytes} B`;
}

function formatCycles(value: number | null | undefined): string {
  if (!value || value <= 0) return '0';
  return Math.round(value).toLocaleString();
}

function formatFeeShannons(value: number | null | undefined): string {
  if (!value || value <= 0) return 'N/A';
  return `${value.toLocaleString()} sh`;
}

function formatFeeDelta(value: number): string {
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(2)} sh/B`;
}

function healthStatusLabel(status: HealthStatus): string {
  if (status === 'healthy') return 'Healthy';
  if (status === 'watch') return 'Watch';
  return 'Critical';
}

function healthBadgeClass(status: HealthStatus): string {
  if (status === 'healthy') return 'bg-emerald-500/15 text-emerald-200 ring-emerald-500/40';
  if (status === 'watch') return 'bg-amber-500/15 text-amber-200 ring-amber-500/45';
  return 'bg-rose-500/15 text-rose-200 ring-rose-500/45';
}

function healthBarClass(status: HealthStatus): string {
  if (status === 'healthy') return 'from-emerald-400 to-emerald-500';
  if (status === 'watch') return 'from-amber-400 to-orange-500';
  return 'from-rose-400 to-rose-500';
}

function metricStress(value: number, warn: number, critical: number): number {
  if (warn <= 0 || critical <= warn) return 0;

  if (value <= warn) return clamp((value / warn) * 0.45, 0, 0.45);
  if (value <= critical) return 0.45 + ((value - warn) / (critical - warn)) * 0.35;

  return 0.8 + clamp((value - critical) / critical, 0, 1) * 0.2;
}

function buildHealthMetric(config: HealthMetricConfig, thresholds: ThresholdPair): HealthMetric {
  const { key, label, value, valueLabel, deltaFormatter } = config;
  const { warn, critical } = thresholds;
  const status: HealthStatus = value >= critical ? 'critical' : value >= warn ? 'watch' : 'healthy';
  const warningGap = warn - value;
  const criticalGap = critical - value;
  const warningDistanceLabel = deltaFormatter(Math.abs(warningGap));
  const criticalDistanceLabel = deltaFormatter(Math.abs(criticalGap));

  return {
    key,
    label,
    value,
    warn,
    critical,
    valueLabel,
    status,
    progressPercent: clamp((value / critical) * 100, 0, 100),
    deltaLabel:
      status === 'healthy'
        ? `${warningDistanceLabel} to warning`
        : status === 'watch'
          ? `${criticalDistanceLabel} to critical`
          : `${criticalDistanceLabel} above critical`,
    warningGap,
    warningDistanceLabel,
    warnLabel: deltaFormatter(warn),
    criticalLabel: deltaFormatter(critical),
  };
}

export function PipelineDashboard({ initialBlocks = [] }: PipelineDashboardProps) {
  const [metricHistory, setMetricHistory] =
    useState<Record<HealthMetricKey, number[]>>(EMPTY_HEALTH_HISTORY);
  const [feeHistory, setFeeHistory] = useState<Record<string, number[]>>({});

  const { data: mempoolInfo } = useQuery({
    queryKey: ['pipeline-dashboard-mempool-info'],
    queryFn: () => api.getMempoolInfo(),
    refetchInterval: 5000,
  });

  const { data: recommendedFees } = useQuery({
    queryKey: ['pipeline-dashboard-recommended-fees'],
    queryFn: () => api.getRecommendedFees(),
    refetchInterval: 5000,
  });

  const { data: pendingProposalsData } = useQuery({
    queryKey: ['pipeline-dashboard-pending-proposals'],
    queryFn: () => api.getPendingProposals(),
    refetchInterval: 5000,
  });

  const proposals = pendingProposalsData?.proposals ?? [];

  const feeEntries = useMemo<FeerateEntry[]>(
    () => [
      {
        label: 'Fastest',
        shortLabel: 'F',
        value: recommendedFees?.fastestFee,
        tone: 'from-rose-400 to-fuchsia-400',
        color: '#f43f5e',
      },
      {
        label: 'Half Hour',
        shortLabel: '30m',
        value: recommendedFees?.halfHourFee,
        tone: 'from-orange-400 to-amber-400',
        color: '#f59e0b',
      },
      {
        label: '1 Hour',
        shortLabel: '1h',
        value: recommendedFees?.hourFee,
        tone: 'from-amber-400 to-yellow-400',
        color: '#facc15',
      },
      {
        label: 'Economy',
        shortLabel: 'Eco',
        value: recommendedFees?.economyFee,
        tone: 'from-cyan-400 to-sky-400',
        color: '#22d3ee',
      },
      {
        label: 'Minimum',
        shortLabel: 'Min',
        value: recommendedFees?.minimumFee,
        tone: 'from-emerald-400 to-teal-400',
        color: '#34d399',
      },
    ],
    [recommendedFees]
  );

  useEffect(() => {
    if (!recommendedFees) return;

    setFeeHistory((prev) => {
      const next: Record<string, number[]> = { ...prev };

      for (const entry of feeEntries) {
        const value = entry.value ?? 0;
        const previousSeries = prev[entry.label] ?? [];

        if (value > 0) {
          next[entry.label] = [...previousSeries, value].slice(-FEE_HISTORY_WINDOW);
        } else {
          next[entry.label] = previousSeries;
        }
      }

      return next;
    });
  }, [feeEntries, recommendedFees]);

  const maxFeerate = useMemo(() => {
    const values = feeEntries.map((entry) => entry.value ?? 0);
    return Math.max(...values, 1);
  }, [feeEntries]);

  const nonZeroFeerates = useMemo(
    () => feeEntries.map((entry) => entry.value ?? 0).filter((value) => value > 0),
    [feeEntries]
  );

  const baseFeerate = useMemo(() => {
    if (nonZeroFeerates.length === 0) return null;
    return Math.min(...nonZeroFeerates);
  }, [nonZeroFeerates]);

  const feerateSpread = useMemo(() => {
    if (nonZeroFeerates.length < 2) return null;
    const maxValue = Math.max(...nonZeroFeerates);
    const minValue = Math.min(...nonZeroFeerates);
    if (minValue <= 0) return null;
    return maxValue / minValue;
  }, [nonZeroFeerates]);

  const proposalInsights = useMemo(() => {
    if (proposals.length === 0) {
      return {
        nearExpiryCount: 0,
        avgBlocksUntilExpiry: 0,
        avgFeeRate: null as number | null,
        nearExpiryItems: [] as typeof proposals,
      };
    }

    const nearExpiryItems = [...proposals]
      .filter((item) => item.blocksUntilExpiry <= 3)
      .sort(
        (a, b) => a.blocksUntilExpiry - b.blocksUntilExpiry || a.proposedAtBlock - b.proposedAtBlock
      )
      .slice(0, 6);

    const avgBlocksUntilExpiry =
      proposals.reduce((sum, item) => sum + item.blocksUntilExpiry, 0) / proposals.length;

    const validFeeRates = proposals
      .map((item) => item.feeRate)
      .filter((value): value is number => value !== null && value !== undefined && value > 0);
    const avgFeeRate =
      validFeeRates.length > 0
        ? validFeeRates.reduce((sum, value) => sum + value, 0) / validFeeRates.length
        : null;

    return {
      nearExpiryCount: nearExpiryItems.length,
      avgBlocksUntilExpiry,
      avgFeeRate,
      nearExpiryItems,
    };
  }, [proposals]);

  const baseHealthMetrics = useMemo<HealthMetricConfig[]>(() => {
    const pendingCount = mempoolInfo?.pendingCount ?? 0;
    const proposedCount = mempoolInfo?.proposedCount ?? 0;
    const queuePressure = pendingCount / Math.max(proposedCount, 1);
    const sizeMb = (mempoolInfo?.totalSize ?? 0) / 1_000_000;
    const cyclesInMillions = (mempoolInfo?.totalCycles ?? 0) / 1_000_000;
    const minFeerate = mempoolInfo?.minFeeRate ?? 0;

    return [
      {
        key: 'pending-queue',
        label: 'Pending queue',
        value: pendingCount,
        defaultWarn: 5_000,
        defaultCritical: 12_000,
        valueLabel: pendingCount.toLocaleString(),
        deltaFormatter: (value) => `${Math.round(value).toLocaleString()} txns`,
      },
      {
        key: 'queue-pressure',
        label: 'Queue pressure',
        value: queuePressure,
        defaultWarn: 4,
        defaultCritical: 8,
        valueLabel: `${queuePressure.toFixed(2)}x`,
        deltaFormatter: (value) => `${value.toFixed(2)}x`,
      },
      {
        key: 'mempool-size',
        label: 'Mempool size',
        value: sizeMb,
        defaultWarn: 4,
        defaultCritical: 10,
        valueLabel: `${sizeMb.toFixed(2)} MB`,
        deltaFormatter: (value) => `${value.toFixed(2)} MB`,
      },
      {
        key: 'mempool-cycles',
        label: 'Mempool cycles',
        value: cyclesInMillions,
        defaultWarn: 250,
        defaultCritical: 700,
        valueLabel: `${cyclesInMillions.toFixed(1)}M`,
        deltaFormatter: (value) => `${value.toFixed(1)}M`,
      },
      {
        key: 'min-feerate',
        label: 'Min fee rate',
        value: minFeerate,
        defaultWarn: 2,
        defaultCritical: 6,
        valueLabel: `${minFeerate.toFixed(2)} sh/B`,
        deltaFormatter: (value) => `${value.toFixed(2)} sh/B`,
      },
    ];
  }, [mempoolInfo]);

  useEffect(() => {
    if (!mempoolInfo) return;

    setMetricHistory((prev) => {
      const next = { ...prev };

      for (const metric of baseHealthMetrics) {
        const previousSeries = prev[metric.key] ?? [];
        const updatedSeries = [...previousSeries, metric.value].slice(-HEALTH_HISTORY_WINDOW);
        next[metric.key] = updatedSeries;
      }

      return next;
    });
  }, [baseHealthMetrics, mempoolInfo?.lastUpdatedAt, mempoolInfo]);

  const healthMetrics = useMemo(
    () =>
      baseHealthMetrics.map((metric) => {
        const thresholds = resolveAdaptiveThresholds(
          metricHistory[metric.key] ?? [],
          metric.defaultWarn,
          metric.defaultCritical
        );

        return buildHealthMetric(metric, thresholds);
      }),
    [baseHealthMetrics, metricHistory]
  );

  const healthScore = useMemo(() => {
    if (healthMetrics.length === 0) return 100;

    const avgStress =
      healthMetrics.reduce(
        (sum, metric) => sum + metricStress(metric.value, metric.warn, metric.critical),
        0
      ) / healthMetrics.length;

    return Math.max(0, Math.round(100 - avgStress * 100));
  }, [healthMetrics]);

  const healthStatus: HealthStatus =
    healthScore >= 75 ? 'healthy' : healthScore >= 50 ? 'watch' : 'critical';

  const distanceToWarning = useMemo(() => {
    const overWarning = [...healthMetrics]
      .filter((metric) => metric.warningGap <= 0)
      .sort((a, b) => a.warningGap - b.warningGap);

    if (overWarning.length > 0) {
      const hotspot = overWarning[0];
      return `Above warning on ${hotspot.label} by ${hotspot.warningDistanceLabel}`;
    }

    const closest = [...healthMetrics].sort((a, b) => a.warningGap - b.warningGap)[0];
    return `${closest.warningDistanceLabel} until ${closest.label} reaches warning`;
  }, [healthMetrics]);

  const panelClassName = 'rounded-2xl bg-slate-900/65 p-4 ring-1 ring-inset ring-slate-800/80';

  return (
    <div className="space-y-5">
      <MempoolBlocks latestBlocks={initialBlocks} chrome="flat" />

      <section className={panelClassName}>
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h2 className="text-base font-semibold text-white sm:text-lg">Recommended Fee Rates</h2>
            <p className="mt-1 text-xs text-slate-400">
              Inclusion priorities mapped as a live fee ladder from minimum to fastest confirmation.
            </p>
          </div>
          {feerateSpread !== null ? (
            <span className="bg-terminal-green/15 text-terminal-green ring-terminal-green/35 rounded-md px-2 py-0.5 text-xs ring-1 ring-inset">
              spread {feerateSpread.toFixed(2)}x
            </span>
          ) : null}
        </div>

        <div className="mt-3 rounded-xl bg-slate-950/60 p-3 ring-1 ring-inset ring-slate-800/70">
          <div className="mb-2 flex items-center justify-between text-[11px] text-slate-500">
            <span className="uppercase tracking-widest">Unified Fee Ladder</span>
            <span>0 - {maxFeerate.toFixed(2)} sh/B</span>
          </div>

          <div className="mb-2.5 grid grid-cols-3 gap-1 text-center text-[10px] uppercase tracking-widest text-slate-500">
            <div className="rounded-sm bg-emerald-500/10 py-0.5 text-emerald-300/80">
              Economy Zone
            </div>
            <div className="rounded-sm bg-amber-500/10 py-0.5 text-amber-300/80">Priority Zone</div>
            <div className="rounded-sm bg-rose-500/10 py-0.5 text-rose-300/80">Urgent Zone</div>
          </div>

          <div className="space-y-2.5">
            {feeEntries.map((entry) => {
              const rawValue = entry.value ?? 0;
              const width = rawValue > 0 ? Math.max((rawValue / maxFeerate) * 100, 8) : 0;
              const markerLeft = clamp(width, 3, 97);
              const multiplier =
                baseFeerate && rawValue > 0 ? `${(rawValue / baseFeerate).toFixed(2)}x min` : 'N/A';
              const historySeries = feeHistory[entry.label] ?? [];
              const trailValues = historySeries.slice(-FEE_TRAIL_POINTS);
              const delta =
                trailValues.length >= 2
                  ? trailValues[trailValues.length - 1] - trailValues[trailValues.length - 2]
                  : 0;
              const hasDelta = trailValues.length >= 2;
              const sparkSeries = historySeries.slice(-FEE_HISTORY_WINDOW);

              return (
                <div
                  key={entry.label}
                  className="grid grid-cols-[auto_1fr_112px] items-center gap-2"
                >
                  <div className="w-24 truncate text-xs text-slate-400">
                    <span className="mr-1 rounded bg-slate-800/90 px-1 py-0.5 text-[10px] uppercase text-slate-300">
                      {entry.shortLabel}
                    </span>
                    {entry.label}
                  </div>

                  <div className="relative h-6 overflow-hidden rounded-md bg-slate-800/80 ring-1 ring-inset ring-slate-700/70">
                    <div className="absolute inset-0 bg-gradient-to-r from-emerald-500/15 via-amber-400/15 to-rose-500/20" />
                    <div
                      className={`absolute left-0 top-0 h-full rounded-md bg-gradient-to-r ${entry.tone}`}
                      style={{ width: `${width}%` }}
                    />
                    {trailValues.map((value, idx) => {
                      const left = clamp((value / maxFeerate) * 100, 3, 97);
                      const isLatest = idx === trailValues.length - 1;
                      const opacity = 0.18 + (idx / Math.max(trailValues.length - 1, 1)) * 0.45;

                      return (
                        <div
                          key={`${entry.label}-trail-${idx}-${value}`}
                          className={`absolute top-1/2 h-1.5 w-1.5 -translate-y-1/2 rounded-full ${
                            isLatest ? 'bg-white/90' : 'bg-white/55'
                          }`}
                          style={{ left: `calc(${left}% - 3px)`, opacity }}
                        />
                      );
                    })}
                    {rawValue > 0 ? (
                      <div
                        className="absolute top-1/2 h-2.5 w-2.5 -translate-y-1/2 rounded-full border border-white/70 bg-slate-100 shadow-[0_0_12px_rgba(226,232,240,0.45)]"
                        style={{ left: `calc(${markerLeft}% - 5px)` }}
                        title={`${entry.label}: ${formatShannonsPerByte(entry.value)}`}
                      />
                    ) : null}
                    <div className="absolute right-1 top-1/2 -translate-y-1/2 text-[11px] font-medium text-slate-100">
                      {formatShannonsPerByte(entry.value)}
                    </div>
                  </div>

                  <div className="w-24 text-right text-[11px] text-slate-500">
                    <div>{multiplier}</div>
                    <div
                      className={
                        hasDelta ? (delta >= 0 ? 'text-emerald-300/90' : 'text-rose-300/90') : ''
                      }
                    >
                      {hasDelta ? formatFeeDelta(delta) : 'n/a'}
                    </div>
                    {sparkSeries.length > 0 ? (
                      <SparkChart
                        data={sparkSeries}
                        color={entry.color}
                        height={20}
                        className="mt-1 h-5"
                      />
                    ) : (
                      <div className="mt-1 h-5 rounded bg-slate-800/40" />
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          <div className="mt-2 text-[11px] text-slate-500">
            Min anchor: {formatShannonsPerByte(baseFeerate)}
          </div>
        </div>
      </section>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="space-y-5">
          <ChainWave initialBlocks={initialBlocks} showHeader={false} chrome="flat" />
        </div>

        <aside className="space-y-4">
          <section className={panelClassName}>
            <div className="flex items-center justify-between gap-2">
              <h2 className="text-sm font-semibold text-white sm:text-base">Mempool Health</h2>
              <span
                className={`rounded-md px-2 py-0.5 text-xs ring-1 ring-inset ${healthBadgeClass(healthStatus)}`}
              >
                {healthStatusLabel(healthStatus)}
              </span>
            </div>

            <div className="mt-3 rounded-xl bg-slate-950/65 p-3 ring-1 ring-inset ring-slate-800/70">
              <div className="flex items-end justify-between gap-2">
                <div>
                  <div className="text-[11px] uppercase tracking-widest text-slate-500">
                    Health Score
                  </div>
                  <div className="text-2xl font-bold text-white">{healthScore}</div>
                </div>
                <div className="text-right text-xs text-slate-400">Distance to warning</div>
              </div>

              <div className="mt-2 h-2 overflow-hidden rounded-full bg-slate-800/80">
                <div
                  className={`h-full rounded-full bg-gradient-to-r ${healthBarClass(healthStatus)}`}
                  style={{ width: `${healthScore}%` }}
                />
              </div>

              <div className="mt-2 text-xs text-slate-400">{distanceToWarning}</div>
              <div className="mt-1 text-[11px] text-slate-500">
                Adaptive thresholds from recent {HEALTH_HISTORY_WINDOW} samples.
              </div>
            </div>

            <div className="mt-3 grid gap-2 text-xs">
              {healthMetrics.map((metric) => (
                <div
                  key={metric.key}
                  className="rounded-lg bg-slate-950/65 p-2 ring-1 ring-inset ring-slate-800/70"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-slate-400">{metric.label}</span>
                    <span className="font-medium text-slate-100">{metric.valueLabel}</span>
                  </div>
                  <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-slate-800/80">
                    <div
                      className={`h-full rounded-full bg-gradient-to-r ${healthBarClass(metric.status)}`}
                      style={{ width: `${metric.progressPercent}%` }}
                    />
                  </div>
                  <div className="mt-1 text-[11px] text-slate-500">{metric.deltaLabel}</div>
                  <div className="text-[11px] text-slate-600">
                    warn {metric.warnLabel} · critical {metric.criticalLabel}
                  </div>
                </div>
              ))}
            </div>

            <div className="mt-2 text-xs text-slate-500">
              Pending: {(mempoolInfo?.pendingCount ?? 0).toLocaleString()} · Proposed:{' '}
              {(mempoolInfo?.proposedCount ?? 0).toLocaleString()} · Total Size:{' '}
              {formatBytes(mempoolInfo?.totalSize)} · Total Cycles:{' '}
              {formatCycles(mempoolInfo?.totalCycles)}
            </div>
          </section>

          <section className={panelClassName}>
            <div className="flex items-center justify-between gap-2">
              <h2 className="text-sm font-semibold text-white sm:text-base">Proposal Pressure</h2>
              <span className="rounded-md bg-amber-500/15 px-2 py-0.5 text-xs text-amber-200 ring-1 ring-inset ring-amber-500/35">
                {proposals.length.toLocaleString()} total
              </span>
            </div>

            <div className="mt-3 grid grid-cols-2 gap-2 text-xs">
              <div className="rounded-lg bg-slate-950/65 px-2 py-1.5">
                <div className="text-slate-500">≤ 3 blocks to expiry</div>
                <div className="font-semibold text-rose-300">
                  {proposalInsights.nearExpiryCount.toLocaleString()}
                </div>
              </div>
              <div className="rounded-lg bg-slate-950/65 px-2 py-1.5">
                <div className="text-slate-500">Avg expiry window</div>
                <div className="font-medium text-slate-100">
                  {proposalInsights.avgBlocksUntilExpiry.toFixed(1)} blocks
                </div>
              </div>
              <div className="col-span-2 rounded-lg bg-slate-950/65 px-2 py-1.5">
                <div className="text-slate-500">Avg proposal fee rate</div>
                <div className="font-medium text-slate-100">
                  {formatShannonsPerByte(proposalInsights.avgFeeRate)}
                </div>
              </div>
            </div>

            <div className="mt-3 rounded-xl bg-slate-950/60 p-2 ring-1 ring-inset ring-slate-800/70">
              <div className="mb-2 text-[11px] uppercase tracking-widest text-slate-500">
                Near expiry queue
              </div>
              {proposalInsights.nearExpiryItems.length === 0 ? (
                <div className="px-1 py-2 text-xs text-slate-500">No near-expiry proposals</div>
              ) : (
                <div className="space-y-1.5">
                  {proposalInsights.nearExpiryItems.map((item) => (
                    <div
                      key={item.proposalId}
                      className="rounded-lg bg-slate-900/55 px-2 py-1.5 text-xs ring-1 ring-inset ring-slate-800/70"
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-mono text-slate-300">
                          {item.fullTxHash
                            ? `${item.fullTxHash.slice(0, 10)}...${item.fullTxHash.slice(-6)}`
                            : `${item.proposalId.slice(0, 10)}...${item.proposalId.slice(-6)}`}
                        </span>
                        <span className="text-rose-300">{item.blocksUntilExpiry} blocks</span>
                      </div>
                      <div className="mt-1 flex items-center justify-between text-[11px] text-slate-500">
                        <span>Fee: {formatFeeShannons(item.fee)}</span>
                        <span>Rate: {formatShannonsPerByte(item.feeRate)}</span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <Link
              href="/charts"
              className="text-terminal-green hover:text-terminal-green/80 mt-3 inline-flex text-xs transition-colors"
            >
              Open charts for broader context
            </Link>
          </section>
        </aside>
      </div>
    </div>
  );
}
