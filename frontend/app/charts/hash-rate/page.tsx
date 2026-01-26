'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function HashRatePage() {
  return <ChartPage title="Hash Rate" queryKey="chart-hash-rate" queryFn={api.getHashRateChart} />;
}
