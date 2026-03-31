'use client';

import { useCallback, useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { api } from '@/lib/api';

export function useCyclesCalculation(
  hash: string,
  txCycles: number | null | undefined,
  isCellbase: boolean,
  cyclesStatus?: string | null
) {
  const queryClient = useQueryClient();
  const [isCalculating, setIsCalculating] = useState(false);
  const [hasFailed, setHasFailed] = useState(false);
  const [triggeredForHash, setTriggeredForHash] = useState<string | null>(null);
  const [resolvedCycles, setResolvedCycles] = useState<number | null>(null);

  const parsedCycles = txCycles ?? null;
  const effectiveCycles = parsedCycles ?? resolvedCycles;
  const hasCycles = effectiveCycles !== null && effectiveCycles > 0;
  const needsCalculation = cyclesStatus === 'pending' && !hasFailed;
  const displayCalculating = needsCalculation && !hasCycles;
  const invalidateTransactionQuery = useCallback(
    () => queryClient.invalidateQueries({ queryKey: ['transaction', hash] }),
    [queryClient, hash]
  );

  useEffect(() => {
    setIsCalculating(false);
    setHasFailed(cyclesStatus === 'failed');
    setTriggeredForHash(null);
    setResolvedCycles(null);
  }, [hash, cyclesStatus]);

  useEffect(() => {
    if (!needsCalculation || triggeredForHash === hash) return;

    setTriggeredForHash(hash);
    setIsCalculating(true);

    const trigger = async () => {
      try {
        const response = await api.triggerCyclesCalculation(hash);

        if (response.status === 'done') {
          await invalidateTransactionQuery();
          if (response.cycles !== null && response.cycles > 0) {
            setResolvedCycles(response.cycles);
            setIsCalculating(false);
          } else {
            setIsCalculating(true);
          }
        } else if (response.status === 'calculating' || response.status === 'queued') {
          setIsCalculating(true);
          await invalidateTransactionQuery();
        } else if (response.status === 'failed' || response.status === 'notFound') {
          setIsCalculating(false);
          setHasFailed(true);
        }
      } catch {
        setIsCalculating(false);
        setHasFailed(true);
      }
    };

    trigger();
  }, [needsCalculation, triggeredForHash, hash, invalidateTransactionQuery]);

  useEffect(() => {
    if (!isCalculating) return;

    const pollInterval = setInterval(async () => {
      try {
        const response = await api.getCyclesStatus(hash);

        if (response.status === 'done') {
          await invalidateTransactionQuery();
          if (response.cycles !== null && response.cycles > 0) {
            setResolvedCycles(response.cycles);
            setIsCalculating(false);
          } else {
            setIsCalculating(true);
          }
        } else if (response.status === 'calculating' || response.status === 'queued') {
          await invalidateTransactionQuery();
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
  }, [isCalculating, hash, invalidateTransactionQuery]);

  useEffect(() => {
    if (hasCycles && isCalculating) {
      setIsCalculating(false);
    }
  }, [hasCycles, isCalculating]);

  return {
    cycles: effectiveCycles,
    hasCycles,
    isCalculating: displayCalculating,
    hasFailed,
  };
}
