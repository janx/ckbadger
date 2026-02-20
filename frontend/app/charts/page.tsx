'use client';

import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { LineChart, LineChartType } from '@/components/ui/line-chart';
import { PieChart } from '@/components/ui/pie-chart';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { ChartCard, ChartSection } from '@/components/ui/chart-card';
import { PageHeader } from '@/components/ui/page-header';
import {
  api,
  ChartResponse,
  MinerDistributionResponse,
  MostUtilizedAssetsChartResponse,
  MostUtilizedScriptsChartResponse,
  StackedAreaChartResponse,
} from '@/lib/api';

function ChartDataWarning({ show }: { show: boolean }) {
  if (!show) return null;
  return (
    <div className="mb-6 rounded border border-yellow-500/30 bg-yellow-500/10 px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="text-yellow-500">⚠</span>
        <span className="font-mono text-sm text-yellow-500">
          Chart data may be incomplete. The indexer is still syncing historical statistics.
        </span>
      </div>
    </div>
  );
}

function LineChartPreview({
  data,
  href,
  chartType = 'line',
}: {
  data: ChartResponse | undefined;
  href: string;
  chartType?: LineChartType;
}) {
  return (
    <ChartCard
      title={data?.title ?? 'Loading...'}
      href={href}
      isLoading={!data}
      error={data === null}
    >
      {data && (
        <LineChart
          data={data.data}
          yAxisLabel={data.yAxisLabel}
          y2AxisLabel={data.y2AxisLabel}
          height={160}
          interactive={false}
          chartType={chartType}
        />
      )}
    </ChartCard>
  );
}

function StackedAreaPreview({
  data,
  href,
  isPercentage = false,
}: {
  data: StackedAreaChartResponse | undefined;
  href: string;
  isPercentage?: boolean;
}) {
  return (
    <ChartCard
      title={data?.title ?? 'Loading...'}
      href={href}
      isLoading={!data}
      error={data === null}
    >
      {data && (
        <StackedAreaChart
          data={data.data}
          series={data.series}
          height={160}
          interactive={false}
          isPercentage={isPercentage}
        />
      )}
    </ChartCard>
  );
}

function MultiSeriesPreview({
  data,
  href,
  defaultSeries = 'liveCells',
}: {
  data: StackedAreaChartResponse | undefined;
  href: string;
  defaultSeries?: string;
}) {
  const chartData: ChartResponse | undefined = data
    ? {
        data: data.data.map((d) => ({
          date: d.date,
          value: d.values[defaultSeries] || '0',
        })),
        title: data.title,
        yAxisLabel: 'Cells',
      }
    : undefined;

  return (
    <ChartCard
      title={data?.title ?? 'Loading...'}
      href={href}
      isLoading={!data}
      error={data === null}
    >
      {chartData && (
        <LineChart
          data={chartData.data}
          yAxisLabel={chartData.yAxisLabel}
          height={160}
          interactive={false}
          primaryColor="#00c389"
        />
      )}
    </ChartCard>
  );
}

function MinerDistributionPreview({
  data,
  href,
}: {
  data: MinerDistributionResponse | undefined;
  href: string;
}) {
  const pieData = data
    ? (() => {
        const items = data.data.slice(0, 8).map((m) => ({
          label: m.minerName || `${m.address.slice(0, 8)}...${m.address.slice(-6)}`,
          value: parseFloat(m.percentage),
        }));
        const othersPercentage = data.data
          .slice(8)
          .reduce((sum, m) => sum + parseFloat(m.percentage), 0);
        if (othersPercentage > 0) {
          items.push({ label: 'Others', value: othersPercentage });
        }
        return items;
      })()
    : [];

  return (
    <ChartCard
      title={data?.title ?? 'Loading...'}
      href={href}
      isLoading={!data}
      error={data === null}
    >
      {data && (
        <div className="flex justify-center">
          <PieChart data={pieData} size={160} showLegend={false} />
        </div>
      )}
    </ChartCard>
  );
}

function MostUtilizedScriptsPreview({
  data,
  href,
}: {
  data: MostUtilizedScriptsChartResponse | undefined;
  href: string;
}) {
  return (
    <ChartCard
      title={data?.title ?? 'Loading...'}
      href={href}
      isLoading={!data}
      error={data === null}
      height={170}
    >
      {data && (
        <StackedAreaChart
          data={data.occupiedShare.data}
          series={data.occupiedShare.series}
          height={160}
          interactive={false}
          isPercentage
          valueUnit="shannon"
        />
      )}
    </ChartCard>
  );
}

function MostUtilizedAssetsPreview({
  data,
  href,
}: {
  data: MostUtilizedAssetsChartResponse | undefined;
  href: string;
}) {
  return (
    <ChartCard
      title={data?.title ?? 'Loading...'}
      href={href}
      isLoading={!data}
      error={data === null}
      height={170}
    >
      {data && (
        <StackedAreaChart
          data={data.occupiedShare.data}
          series={data.occupiedShare.series}
          height={160}
          interactive={false}
          isPercentage
          valueUnit="shannon"
        />
      )}
    </ChartCard>
  );
}

export default function ChartsPage() {
  const { data: networkStats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
  });

  const { data: totalDeposit } = useQuery({
    queryKey: ['dao-chart-total-deposit'],
    queryFn: () => api.getDaoTotalDepositChart(),
  });

  const { data: dailyDeposit } = useQuery({
    queryKey: ['dao-chart-daily-deposit'],
    queryFn: () => api.getDaoDailyDepositChart(),
  });

  const { data: circulationRatio } = useQuery({
    queryKey: ['dao-chart-circulation-ratio'],
    queryFn: () => api.getDaoCirculationRatioChart(),
  });

  const { data: transactionCount } = useQuery({
    queryKey: ['chart-transaction-count'],
    queryFn: () => api.getTransactionCountChart(),
  });

  const { data: cellCount } = useQuery({
    queryKey: ['chart-cell-count'],
    queryFn: () => api.getCellCountChart(),
  });

  const { data: knowledgeSize } = useQuery({
    queryKey: ['chart-knowledge-size'],
    queryFn: () => api.getKnowledgeSizeChart(),
  });

  const { data: commonKnowledgeComposition } = useQuery({
    queryKey: ['chart-common-knowledge-composition'],
    queryFn: () => api.getCommonKnowledgeCompositionChart(),
  });

  const { data: cellAgeVsOccupiedCapacity } = useQuery({
    queryKey: ['chart-cell-age-vs-occupied-capacity'],
    queryFn: () => api.getCellAgeVsOccupiedCapacityChart(),
  });

  const { data: capacityTurnoverRatio } = useQuery({
    queryKey: ['chart-capacity-turnover-ratio'],
    queryFn: () => api.getCapacityTurnoverRatioChart(),
  });

  const { data: cellSizeDistribution } = useQuery({
    queryKey: ['chart-cell-size-distribution'],
    queryFn: () => api.getCellSizeDistributionChart(),
  });

  const { data: addressCohortRetention } = useQuery({
    queryKey: ['chart-address-cohort-retention'],
    queryFn: () => api.getAddressCohortRetentionChart(),
  });

  const { data: mostUtilizedScripts } = useQuery({
    queryKey: ['chart-most-utilized-scripts'],
    queryFn: () => api.getMostUtilizedScriptsChart(),
  });

  const { data: mostUtilizedAssets } = useQuery({
    queryKey: ['chart-most-utilized-assets'],
    queryFn: () => api.getMostUtilizedAssetsChart(),
  });

  const { data: blockTimeDistribution } = useQuery({
    queryKey: ['chart-block-time-distribution'],
    queryFn: () => api.getBlockTimeDistributionChart(),
  });

  const { data: epochTimeDistribution } = useQuery({
    queryKey: ['chart-epoch-time-distribution'],
    queryFn: () => api.getEpochTimeDistributionChart(),
  });

  const { data: averageBlockTime } = useQuery({
    queryKey: ['chart-average-block-time'],
    queryFn: () => api.getAverageBlockTimeChart(),
  });

  const { data: epochTimeLength } = useQuery({
    queryKey: ['chart-epoch-time-length'],
    queryFn: () => api.getEpochTimeLengthChart(),
  });

  const { data: hashRate } = useQuery({
    queryKey: ['chart-hash-rate'],
    queryFn: () => api.getHashRateChart(),
  });

  const { data: difficulty } = useQuery({
    queryKey: ['chart-difficulty'],
    queryFn: () => api.getDifficultyChart(),
  });

  const { data: uncleRate } = useQuery({
    queryKey: ['chart-uncle-rate'],
    queryFn: () => api.getUncleRateChart(),
  });

  const { data: minerDistribution } = useQuery({
    queryKey: ['chart-miner-address-distribution'],
    queryFn: () => api.getMinerAddressDistributionChart(),
  });

  const { data: totalSupply } = useQuery({
    queryKey: ['chart-total-supply'],
    queryFn: () => api.getTotalSupplyChart(),
  });

  const { data: nominalApc } = useQuery({
    queryKey: ['chart-nominal-apc'],
    queryFn: () => api.getNominalApcChart(),
  });

  const { data: secondaryIssuance } = useQuery({
    queryKey: ['chart-secondary-issuance'],
    queryFn: () => api.getSecondaryIssuanceChart(),
  });

  const { data: inflationRate } = useQuery({
    queryKey: ['chart-inflation-rate'],
    queryFn: () => api.getInflationRateChart(),
  });

  const { data: hodlWave } = useQuery({
    queryKey: ['chart-hodl-wave'],
    queryFn: () => api.getHodlWaveChart(),
  });

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader title="Charts" subtitle="Historical charts and statistics for Nervos CKB" />

        <ChartDataWarning show={networkStats?.syncStatus?.chartDataMayBeIncomplete ?? false} />

        <ChartSection title="Proof of Work">
          <LineChartPreview data={hashRate} href="/charts/hash-rate" />
          <LineChartPreview data={difficulty} href="/charts/difficulty" />
          <LineChartPreview data={uncleRate} href="/charts/uncle-rate" />
          <MinerDistributionPreview
            data={minerDistribution}
            href="/charts/miner-address-distribution"
          />
        </ChartSection>

        <ChartSection title="Nervos DAO">
          <LineChartPreview data={totalDeposit} href="/charts/total-deposit" />
          <LineChartPreview data={dailyDeposit} href="/charts/daily-deposit" />
          <LineChartPreview data={circulationRatio} href="/charts/circulation-ratio" />
        </ChartSection>

        <ChartSection title="Block">
          <LineChartPreview data={blockTimeDistribution} href="/charts/block-time-distribution" />
          <LineChartPreview data={epochTimeDistribution} href="/charts/epoch-time-distribution" />
          <LineChartPreview data={epochTimeLength} href="/charts/epoch-time-length" />
          <LineChartPreview data={averageBlockTime} href="/charts/average-block-time" />
        </ChartSection>

        <ChartSection title="Activities">
          <LineChartPreview data={transactionCount} href="/charts/transaction-count" />
          <MultiSeriesPreview
            data={cellCount}
            href="/charts/cell-count"
            defaultSeries="liveCells"
          />
          <StackedAreaPreview data={hodlWave} href="/charts/hodl-wave" isPercentage />
        </ChartSection>

        <ChartSection title="Common Knowledge Bytes">
          <LineChartPreview data={knowledgeSize} href="/charts/knowledge-size" />
          <StackedAreaPreview
            data={commonKnowledgeComposition}
            href="/charts/common-knowledge-composition"
          />
          <StackedAreaPreview
            data={cellAgeVsOccupiedCapacity}
            href="/charts/cell-age-vs-occupied-capacity"
          />
          <LineChartPreview data={capacityTurnoverRatio} href="/charts/capacity-turnover-ratio" />
          <LineChartPreview
            data={cellSizeDistribution}
            href="/charts/cell-size-distribution"
            chartType="bar"
          />
          <LineChartPreview
            data={addressCohortRetention}
            href="/charts/address-cohort-retention"
            chartType="bar"
          />
          <MostUtilizedScriptsPreview
            data={mostUtilizedScripts}
            href="/charts/most-utilized-scripts"
          />
          <MostUtilizedAssetsPreview
            data={mostUtilizedAssets}
            href="/charts/most-utilized-assets"
          />
        </ChartSection>

        <ChartSection title="Economics">
          <StackedAreaPreview data={totalSupply} href="/charts/total-supply" />
          <LineChartPreview data={nominalApc} href="/charts/nominal-apc" />
          <StackedAreaPreview
            data={secondaryIssuance}
            href="/charts/secondary-issuance"
            isPercentage
          />
          <LineChartPreview data={inflationRate} href="/charts/inflation-rate" />
        </ChartSection>
      </main>
    </div>
  );
}
