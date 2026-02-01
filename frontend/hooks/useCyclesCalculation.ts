'use client';

import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { api } from '@/lib/api';

export function useCyclesCalculation(
  hash: string,
  txCycles: number | undefined,
  isCellbase: boolean
) {
  const queryClient = useQueryClient();
  const [isCalculating, setIsCalculating] = useState(false);
  const [hasFailed, setHasFailed] = useState(false);
  const [triggeredForHash, setTriggeredForHash] = useState<string | null>(null);

  const parsedCycles = txCycles ?? null;
  const hasCycles = parsedCycles !== null && parsedCycles > 0;
  const needsCalculation = !isCellbase && !hasCycles && !hasFailed;

  useEffect(() => {
    setIsCalculating(false);
    setHasFailed(false);
    setTriggeredForHash(null);
  }, [hash]);

  useEffect(() => {
    if (!needsCalculation || triggeredForHash === hash) return;

    setTriggeredForHash(hash);

    const trigger = async () => {
      try {
        const response = await api.triggerCyclesCalculation(hash);

        if (response.status === 'done') {
          queryClient.invalidateQueries({ queryKey: ['transaction', hash] });
        } else if (response.status === 'failed' || response.status === 'notFound') {
          setHasFailed(true);
        } else {
          setIsCalculating(true);
        }
      } catch {
        setHasFailed(true);
      }
    };

    trigger();
  }, [needsCalculation, triggeredForHash, hash, queryClient]);

  useEffect(() => {
    if (!isCalculating) return;

    const pollInterval = setInterval(async () => {
      try {
        const response = await api.getCyclesStatus(hash);

        if (response.status === 'done') {
          setIsCalculating(false);
          queryClient.invalidateQueries({ queryKey: ['transaction', hash] });
        } else if (response.status === 'failed' || response.status === 'notFound') {
          setIsCalculating(false);
          setHasFailed(true);
        }
      } catch {
        setIsCalculating(false);
        setHasFailed(true);
      }
    }, 2000);

    return () => clearInterval(pollInterval);
  }, [isCalculating, hash, queryClient]);

  return {
    cycles: parsedCycles,
    hasCycles,
    isCalculating,
    hasFailed,
  };
}
