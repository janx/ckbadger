'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function CellCountPage() {
  return (
    <ChartPage
      title="Live Cell Count"
      queryKey="chart-cell-count"
      queryFn={api.getCellCountChart}
    />
  );
}
