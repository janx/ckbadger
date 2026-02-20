'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function CellSizeDistributionPage() {
  return (
    <ChartPage
      title="Cell Size Distribution"
      queryKey="chart-cell-size-distribution"
      queryFn={api.getCellSizeDistributionChart}
    />
  );
}
