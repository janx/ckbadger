'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function AddressCohortRetentionPage() {
  return (
    <ChartPage
      title="Address Cohort Retention"
      queryKey="chart-address-cohort-retention"
      queryFn={api.getAddressCohortRetentionChart}
    />
  );
}
