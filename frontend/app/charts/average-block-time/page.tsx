'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function AverageBlockTimePage() {
  return (
    <ChartPage
      title="Average Block Time"
      queryKey="chart-average-block-time"
      queryFn={api.getAverageBlockTimeChart}
    />
  );
}
