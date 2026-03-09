'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { ChartCalculationNote } from '@/components/charts/chart-calculation-note';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
import { PieChart } from '@/components/ui/pie-chart';
import { api, MinerDistributionDataPoint } from '@/lib/api';

function formatNumber(num: number): string {
  return num.toLocaleString();
}

function MinerRow({
  miner,
  rank,
  color,
}: {
  miner: MinerDistributionDataPoint;
  rank: number;
  color: string;
}) {
  const isCkbAddress = miner.address.startsWith('ckb1') || miner.address.startsWith('ckt1');
  const addressPath = isCkbAddress
    ? miner.address
    : miner.address.startsWith('0x')
      ? miner.address.slice(2)
      : miner.address;

  return (
    <TerminalRow>
      <div className="flex items-center">
        <div className="text-text-muted w-12 font-mono">{rank}</div>
        <div className="flex flex-1 items-center gap-3">
          <div className="h-3 w-3 flex-shrink-0 rounded" style={{ backgroundColor: color }} />
          <Link href={`/address/${addressPath}`} className="group flex flex-col gap-0.5">
            {miner.minerName && (
              <span className="group-hover:text-emphasis text-sm font-medium text-white transition-colors">
                {miner.minerName}
              </span>
            )}
            <span className="group-hover:text-emphasis text-text-muted font-mono text-sm transition-colors">
              {miner.address.slice(0, 10)}...{miner.address.slice(-8)}
            </span>
          </Link>
        </div>
        <div className="w-28 text-right font-mono text-white">
          {formatNumber(miner.blocksMined)}
        </div>
        <div className="flex w-40 items-center justify-end gap-2">
          <div className="bg-base-elevated h-2 w-20 overflow-hidden rounded-full">
            <div
              className="h-full rounded-full"
              style={{
                width: `${Math.min(parseFloat(miner.percentage), 100)}%`,
                backgroundColor: color,
              }}
            />
          </div>
          <span className="text-text-muted w-16 text-right font-mono text-sm">
            {parseFloat(miner.percentage).toFixed(2)}%
          </span>
        </div>
      </div>
    </TerminalRow>
  );
}

const COLORS = [
  '#8b5cf6',
  '#00ff41',
  '#ffb000',
  '#ef4444',
  '#3b82f6',
  '#ec4899',
  '#14b8a6',
  '#f97316',
  '#6366f1',
  '#84cc16',
  '#a855f7',
  '#22d3ee',
];

export default function MinerAddressDistributionPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-miner-address-distribution'],
    queryFn: api.getMinerAddressDistributionChart,
  });

  const pieData =
    data?.data.slice(0, 10).map((m) => ({
      label: m.minerName || `${m.address.slice(0, 8)}...${m.address.slice(-6)}`,
      value: parseFloat(m.percentage),
    })) || [];

  const othersPercentage =
    data?.data.slice(10).reduce((sum, m) => sum + parseFloat(m.percentage), 0) || 0;
  if (othersPercentage > 0) {
    pieData.push({ label: 'Others', value: othersPercentage });
  }

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-emphasis text-text-muted text-sm transition-colors"
          >
            ← Back to Charts
          </Link>
        </div>

        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">
            <div className="flex items-center gap-4">
              <span>Miner Address Distribution</span>
              {data && (
                <span className="text-text-muted text-sm font-normal">
                  Total Blocks: {formatNumber(data.totalBlocks)}
                </span>
              )}
            </div>
          </TerminalPanelHeader>
          <TerminalPanelContent className="p-6">
            {isLoading && (
              <div className="border-base-border bg-base-surface/50 h-80 animate-pulse rounded border" />
            )}
            {error && (
              <div className="text-text-muted flex h-80 items-center justify-center">
                Failed to load data
              </div>
            )}
            {data && pieData.length > 0 && (
              <div>
                <div className="flex justify-center">
                  <PieChart
                    data={pieData}
                    size={320}
                    showLegend={true}
                    formatValue={(v) => v.toFixed(2) + '%'}
                  />
                </div>
                <ChartCalculationNote
                  description={
                    getChartDescription('chart-miner-address-distribution', {
                      seriesLabels: pieData.map((item) => item.label),
                    }) ?? {
                      overview: 'Shows block-production share by miner.',
                      legendItems: [],
                    }
                  }
                />
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">All Miners</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {data && (
              <>
                <div className="border-base-border bg-base-surface/50 text-text-muted flex border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
                  <div className="w-12">Rank</div>
                  <div className="flex-1">Miner Address</div>
                  <div className="w-28 text-right">Blocks Mined</div>
                  <div className="w-40 text-right">Share</div>
                </div>
                {data.data.map((miner, index) => (
                  <MinerRow
                    key={miner.address}
                    miner={miner}
                    rank={index + 1}
                    color={COLORS[index % COLORS.length]}
                  />
                ))}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
