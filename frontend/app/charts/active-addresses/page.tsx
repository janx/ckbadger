'use client';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function ActiveAddressesPage() {
  return (
    <ChartPage
      title="Daily Active Addresses"
      queryKey="chart-active-addresses"
      queryFn={api.getActiveAddressesChart}
    />
  );
}
