'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function UncleRatePage() {
  return (
    <ChartPage title="Uncle Rate" queryKey="chart-uncle-rate" queryFn={api.getUncleRateChart} />
  );
}
