'use client';

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function EpochTimeLengthPage() {
  const { data: hardforkTimeline } = useQuery({
    queryKey: ['hardforks-for-epoch-time-length'],
    queryFn: () => api.getHardforks(),
    staleTime: 60_000,
  });

  const markers = useMemo(
    () =>
      (hardforkTimeline?.events ?? []).map((event) => ({
        x: String(event.activationEpoch),
        label: event.shortName.toUpperCase(),
        color: event.status === 'activated' ? '#f59e0b' : '#38bdf8',
        href: event.activationBlock ? `/blocks/${event.activationBlock}` : undefined,
      })),
    [hardforkTimeline?.events]
  );

  return (
    <ChartPage
      title="Epoch Time Length"
      queryKey="chart-epoch-time-length"
      queryFn={api.getEpochTimeLengthChart}
      markers={markers}
    />
  );
}
