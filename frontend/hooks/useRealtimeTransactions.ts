'use client';

import { useEffect, useState, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useWebSocket } from './useWebSocket';

interface Transaction {
  hash: string;
  blockNumber: number;
  fee: string;
}

export function useRealtimeTransactions(enabled = true) {
  const queryClient = useQueryClient();
  const [latestTx, setLatestTx] = useState<Transaction | null>(null);

  const handleMessage = useCallback(
    (message: { type: string; data?: unknown }) => {
      if (message.type === 'new_transaction') {
        const txData = message.data as Transaction;
        setLatestTx(txData);

        queryClient.setQueryData(
          ['latest-transactions'],
          (old: { data: Transaction[] } | undefined) => {
            if (!old) return old;
            return {
              ...old,
              data: [txData, ...old.data.slice(0, 9)],
            };
          }
        );
      }
    },
    [queryClient]
  );

  const { isConnected, subscribe, unsubscribe } = useWebSocket({
    onMessage: handleMessage,
    onConnect: () => {
      if (enabled) {
        subscribe('new_transaction');
      }
    },
  });

  useEffect(() => {
    if (isConnected && enabled) {
      subscribe('new_transaction');
    }
    return () => {
      if (isConnected) {
        unsubscribe('new_transaction');
      }
    };
  }, [isConnected, enabled, subscribe, unsubscribe]);

  return { latestTx, isConnected };
}
