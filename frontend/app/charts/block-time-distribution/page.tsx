'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function BlockTimeDistributionPage() {
  return (
    <ChartPage
      title="Block Time Distribution"
      queryKey="chart-block-time-distribution"
      queryFn={api.getBlockTimeDistributionChart}
    />
  );
}
