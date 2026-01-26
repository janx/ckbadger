'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function EpochTimeLengthPage() {
  return (
    <ChartPage
      title="Epoch Time Length"
      queryKey="chart-epoch-time-length"
      queryFn={api.getEpochTimeLengthChart}
    />
  );
}
