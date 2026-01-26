'use client';

import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function KnowledgeSizePage() {
  return (
    <ChartPage
      title="Common Knowledge Size"
      queryKey="chart-knowledge-size"
      queryFn={api.getKnowledgeSizeChart}
    />
  );
}
