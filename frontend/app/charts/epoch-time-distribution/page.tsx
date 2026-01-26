'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function EpochTimeDistributionPage() {
  return (
    <ChartPage
      title="Epoch Time Distribution"
      queryKey="chart-epoch-time-distribution"
      queryFn={api.getEpochTimeDistributionChart}
    />
  );
}
