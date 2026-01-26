'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function NominalApcPage() {
  return (
    <ChartPage
      title="Nominal DAO Compensation Rate"
      queryKey="chart-nominal-apc"
      queryFn={api.getNominalApcChart}
    />
  );
}
