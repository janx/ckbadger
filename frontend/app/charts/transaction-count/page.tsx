'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function TransactionCountPage() {
  return (
    <ChartPage
      title="Transaction Count"
      queryKey="chart-transaction-count"
      queryFn={api.getTransactionCountChart}
    />
  );
}
