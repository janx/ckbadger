'use client';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function ActivityVolumePage() {
  return (
    <ChartPage
      title="Daily Activity Volume"
      queryKey="chart-activity-volume"
      queryFn={api.getActivityVolumeChart}
    />
  );
}
