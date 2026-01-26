'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function DifficultyPage() {
  return (
    <ChartPage title="Difficulty" queryKey="chart-difficulty" queryFn={api.getDifficultyChart} />
  );
}
