'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function InflationRatePage() {
  return (
    <ChartPage
      title="Inflation Rate"
      queryKey="chart-inflation-rate"
      queryFn={api.getInflationRateChart}
    />
  );
}
