'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function TotalDepositPage() {
  return (
    <ChartPage
      title="Total Deposit"
      queryKey="dao-chart-total-deposit"
      queryFn={api.getDaoTotalDepositChart}
    />
  );
}
