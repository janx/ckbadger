'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';

/** Poll interval of the pipeline snapshot; WebSocket blocks and reorgs refetch it sooner. */
export const PIPELINE_POLL_MS = 2000;

/** The homepage transaction pipeline: one backend snapshot, polled and pushed fresh on blocks. */
export function usePipelineSnapshot() {
  return useQuery({
    queryKey: ['pipeline'],
    queryFn: () => api.getPipeline(),
    refetchInterval: PIPELINE_POLL_MS,
  });
}
