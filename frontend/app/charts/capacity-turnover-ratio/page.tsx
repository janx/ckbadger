'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function CapacityTurnoverRatioPage() {
  return (
    <ChartPage
      title="Capacity Turnover Ratio"
      queryKey="chart-capacity-turnover-ratio"
      queryFn={api.getCapacityTurnoverRatioChart}
    />
  );
}
