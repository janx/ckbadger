'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function CirculationRatioPage() {
  return (
    <ChartPage
      title="Deposit to Circulation Ratio"
      queryKey="dao-chart-circulation-ratio"
      queryFn={api.getDaoCirculationRatioChart}
    />
  );
}
