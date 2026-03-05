'use client';

import { create } from 'zustand';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useCallback } from 'react';

interface SyncStatus {
  isSyncing: boolean;
  syncedBlock: number;
  tipBlock: number;
  progress: number;
  estimatedTime: string | null;
  chartDataMayBeIncomplete: boolean;
  blocksPerSecond: number | null;
  emaBlocksPerSecond: number | null;
  txsPerSecond?: number | null;
  emaTxsPerSecond?: number | null;
  syncMode: string;
  startedAt: number | null;
  elapsedTime: string | null;
  totalTime: string | null;
}

interface Block {
  number: number;
  hash: string;
  timestamp: string;
  transactionsCount: number;
  epochNumber: number;
  epochIndex: number;
  epochLength: number;
  avgBlockTime: string;
  estimatedEpochTime: string;
  syncStatus: SyncStatus;
}

interface Transaction {
  hash: string;
  blockNumber: number;
  inputsCount: number;
  outputsCount: number;
  fee: string;
  timestamp: string;
}

interface WebSocketMessage {
  type: string;
  data?: unknown;
}

interface RealtimeState {
  isConnected: boolean;
  latestBlock: Block | null;
  latestTx: Transaction | null;
  setConnected: (connected: boolean) => void;
  setLatestBlock: (block: Block) => void;
  setLatestTx: (tx: Transaction) => void;
}

export const useRealtimeStore = create<RealtimeState>((set) => ({
  isConnected: false,
  latestBlock: null,
  latestTx: null,
  setConnected: (connected) => set({ isConnected: connected }),
  setLatestBlock: (block) => set({ latestBlock: block }),
  setLatestTx: (tx) => set({ latestTx: tx }),
}));

let wsInstance: WebSocket | null = null;
let wsSubscribers = 0;
let reconnectAttempts = 0;
let reconnectTimeout: NodeJS.Timeout | null = null;
const MAX_RECONNECT_ATTEMPTS = 10;
const RECONNECT_INTERVAL = 3000;

type MessageHandler = (message: WebSocketMessage) => void;
const messageHandlers = new Set<MessageHandler>();

function connectWebSocket() {
  if (wsInstance?.readyState === WebSocket.OPEN) return;

  const wsUrl = process.env.NEXT_PUBLIC_WS_URL || 'ws://localhost:8101/ws';

  try {
    wsInstance = new WebSocket(wsUrl);

    wsInstance.onopen = () => {
      useRealtimeStore.getState().setConnected(true);
      reconnectAttempts = 0;
      wsInstance?.send(JSON.stringify({ action: 'subscribe', channel: 'new_block' }));
      wsInstance?.send(JSON.stringify({ action: 'subscribe', channel: 'new_transaction' }));
    };

    wsInstance.onmessage = (event) => {
      try {
        const message = JSON.parse(event.data) as WebSocketMessage;
        messageHandlers.forEach((handler) => handler(message));
      } catch {
        console.error('Failed to parse WebSocket message');
      }
    };

    wsInstance.onclose = () => {
      useRealtimeStore.getState().setConnected(false);
      wsInstance = null;

      if (wsSubscribers > 0 && reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
        reconnectTimeout = setTimeout(() => {
          reconnectAttempts++;
          connectWebSocket();
        }, RECONNECT_INTERVAL);
      }
    };

    wsInstance.onerror = () => {
      wsInstance?.close();
    };
  } catch (error) {
    console.error('WebSocket connection error:', error);
  }
}

function disconnectWebSocket() {
  if (reconnectTimeout) {
    clearTimeout(reconnectTimeout);
    reconnectTimeout = null;
  }
  reconnectAttempts = MAX_RECONNECT_ATTEMPTS;
  wsInstance?.close();
  wsInstance = null;
}

export function useRealtimeData() {
  const queryClient = useQueryClient();
  const { isConnected, latestBlock, latestTx, setLatestBlock, setLatestTx } = useRealtimeStore();
  const handlerRef = useRef<MessageHandler | null>(null);

  const handleMessage = useCallback(
    (message: WebSocketMessage) => {
      if (message.type === 'new_block') {
        const blockData = message.data as Block;
        setLatestBlock(blockData);

        queryClient.setQueryData(['latest-blocks'], (old: { data: Block[] } | undefined) => {
          const existingData = old?.data ?? [];
          const merged = [blockData, ...existingData.filter((b) => b.number !== blockData.number)];
          merged.sort((a, b) => b.number - a.number);
          return {
            ...old,
            data: merged.slice(0, 10),
          };
        });

        queryClient.setQueryData(
          ['network-stats'],
          (
            old:
              | {
                  latestBlock: number;
                  syncStatus?: SyncStatus;
                  epoch?: string;
                  estimatedEpochTime?: string;
                  avgBlockTime?: string;
                }
              | undefined
          ) => {
            if (!old) return old;

            const epochString = `${blockData.epochNumber}(${blockData.epochIndex}/${blockData.epochLength})`;

            return {
              ...old,
              latestBlock: blockData.number,
              epoch: epochString,
              avgBlockTime: blockData.avgBlockTime,
              estimatedEpochTime: blockData.estimatedEpochTime,
              syncStatus: blockData.syncStatus ?? old.syncStatus,
            };
          }
        );

        // Also update ChainWave's tip block cache
        queryClient.setQueryData(['chain-wave-tip-block'], (old: { data: Block[] } | undefined) => {
          if (!old) return old;
          return {
            ...old,
            data: [blockData],
          };
        });
      } else if (message.type === 'new_transaction') {
        const txData = message.data as Transaction;
        setLatestTx(txData);

        queryClient.setQueryData(
          ['latest-transactions'],
          (old: { data: Transaction[] } | undefined) => {
            const existingData = old?.data ?? [];
            // Dedupe by hash and keep max 10 items
            const newData = [txData, ...existingData.filter((t) => t.hash !== txData.hash)];
            return {
              ...old,
              data: newData.slice(0, 10),
            };
          }
        );
      }
    },
    [queryClient, setLatestBlock, setLatestTx]
  );

  useEffect(() => {
    wsSubscribers++;
    handlerRef.current = handleMessage;
    messageHandlers.add(handleMessage);
    connectWebSocket();

    return () => {
      wsSubscribers--;
      if (handlerRef.current) {
        messageHandlers.delete(handlerRef.current);
      }
      if (wsSubscribers === 0) {
        disconnectWebSocket();
      }
    };
  }, [handleMessage]);

  return { isConnected, latestBlock, latestTx };
}
