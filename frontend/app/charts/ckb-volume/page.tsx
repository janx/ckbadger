'use client';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function CkbVolumePage() {
  return (
    <ChartPage
      title="Daily CKB Transfer Volume"
      queryKey="chart-ckb-volume"
      queryFn={api.getCkbVolumeChart}
      chartType="bar"
    />
  );
}
