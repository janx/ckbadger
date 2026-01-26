'use client';

import { useEffect, useState, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useWebSocket } from './useWebSocket';

interface Block {
  number: number;
  hash: string;
  timestamp: string;
  transactionsCount: number;
}

export function useRealtimeBlocks(enabled = true) {
  const queryClient = useQueryClient();
  const [latestBlock, setLatestBlock] = useState<Block | null>(null);

  const handleMessage = useCallback(
    (message: { type: string; data?: unknown }) => {
      if (message.type === 'new_block') {
        const blockData = message.data as Block;
        setLatestBlock(blockData);

        queryClient.setQueryData(['latest-blocks'], (old: { data: Block[] } | undefined) => {
          if (!old) return old;
          const merged = [blockData, ...old.data.filter((b) => b.number !== blockData.number)];
          merged.sort((a, b) => b.number - a.number);
          return {
            ...old,
            data: merged.slice(0, 10),
          };
        });

        queryClient.invalidateQueries({ queryKey: ['network-stats'] });
      }
    },
    [queryClient]
  );

  const { isConnected, subscribe, unsubscribe } = useWebSocket({
    onMessage: handleMessage,
    onConnect: () => {
      if (enabled) {
        subscribe('new_block');
      }
    },
  });

  useEffect(() => {
    if (isConnected && enabled) {
      subscribe('new_block');
    }
    return () => {
      if (isConnected) {
        unsubscribe('new_block');
      }
    };
  }, [isConnected, enabled, subscribe, unsubscribe]);

  return { latestBlock, isConnected };
}
