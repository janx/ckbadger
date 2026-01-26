'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function DailyDepositPage() {
  return (
    <ChartPage
      title="Daily Deposit"
      queryKey="dao-chart-daily-deposit"
      queryFn={api.getDaoDailyDepositChart}
    />
  );
}
