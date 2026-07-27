'use client';

import { create } from 'zustand';
import { wsUrlFor } from '@/lib/active-network';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
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
  reset: () => void;
}

export const useRealtimeStore = create<RealtimeState>((set) => ({
  isConnected: false,
  latestBlock: null,
  latestTx: null,
  setConnected: (connected) => set({ isConnected: connected }),
  setLatestBlock: (block) => set({ latestBlock: block }),
  setLatestTx: (tx) => set({ latestTx: tx }),
  reset: () => set({ isConnected: false, latestBlock: null, latestTx: null }),
}));

let wsInstance: WebSocket | null = null;
// The network the current socket targets, so we can detect a network switch and
// reconnect instead of silently streaming the wrong network's data.
let wsNetwork: string | null = null;
let wsSubscribers = 0;
let reconnectAttempts = 0;
let reconnectTimeout: NodeJS.Timeout | null = null;
const MAX_RECONNECT_ATTEMPTS = 10;
const RECONNECT_INTERVAL = 3000;

type MessageHandler = (message: WebSocketMessage) => void;
const messageHandlers = new Set<MessageHandler>();

function connectWebSocket(network: string) {
  // Already connected to the right network — nothing to do.
  if (wsInstance?.readyState === WebSocket.OPEN && wsNetwork === network) return;

  // A socket exists but targets a different network (or is stale): tear it down
  // WITHOUT letting its onclose auto-reconnect to the old network.
  if (wsInstance) {
    wsInstance.onclose = null;
    wsInstance.onerror = null;
    wsInstance.close();
    wsInstance = null;
  }
  if (reconnectTimeout) {
    clearTimeout(reconnectTimeout);
    reconnectTimeout = null;
  }
  reconnectAttempts = 0;
  wsNetwork = network;

  const wsUrl = wsUrlFor(network);

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
          connectWebSocket(network);
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
  if (wsInstance) {
    // `close()` delivers `onclose` asynchronously, so a still-attached handler
    // would run AFTER a new socket exists: it would null `wsInstance` (orphaning
    // the new socket while its handlers keep feeding `messageHandlers`) and
    // schedule a reconnect to the closure-captured old network. Detach first.
    wsInstance.onclose = null;
    wsInstance.onerror = null;
    wsInstance.close();
    wsInstance = null;
  }
  wsNetwork = null;
}

export function useRealtimeData() {
  const queryClient = useQueryClient();
  const network = useActiveNetwork();
  const { isConnected, latestBlock, latestTx, setLatestBlock, setLatestTx, reset } =
    useRealtimeStore();
  const handlerRef = useRef<MessageHandler | null>(null);
  const previousNetwork = useRef(network);

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

  // Clear stale realtime data (blocks/txs/connection) when the active network
  // changes, so a switched-away network's data isn't shown while the socket
  // reconnects to the new network.
  useEffect(() => {
    if (previousNetwork.current !== network) {
      reset();
      previousNetwork.current = network;
    }
  }, [network, reset]);

  useEffect(() => {
    wsSubscribers++;
    handlerRef.current = handleMessage;
    messageHandlers.add(handleMessage);
    connectWebSocket(network);

    return () => {
      wsSubscribers--;
      if (handlerRef.current) {
        messageHandlers.delete(handlerRef.current);
      }
      if (wsSubscribers === 0) {
        disconnectWebSocket();
      }
    };
  }, [handleMessage, network]);

  return { isConnected, latestBlock, latestTx };
}
