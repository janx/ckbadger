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
  /**
   * Whether the current page holds the realtime socket (mounts useRealtimeData), whatever the
   * socket's state. Pages without it never connect by design. Set only by useRealtimeData, from
   * the same subscriber count that opens and closes the socket; reset() leaves it alone.
   */
  socketHeld: boolean;
  latestBlock: Block | null;
  latestTx: Transaction | null;
  /** Advances for every canonical block signal or reorg, including duplicate heights. */
  chainRevision: number;
  reorgRevision: number;
  setConnected: (connected: boolean) => void;
  setLatestBlock: (block: Block) => void;
  setLatestTx: (tx: Transaction) => void;
  reset: () => void;
}

export const useRealtimeStore = create<RealtimeState>((set) => ({
  isConnected: false,
  socketHeld: false,
  latestBlock: null,
  latestTx: null,
  chainRevision: 0,
  reorgRevision: 0,
  setConnected: (connected) => set({ isConnected: connected }),
  setLatestBlock: (block) => set({ latestBlock: block }),
  setLatestTx: (tx) => set({ latestTx: tx }),
  reset: () =>
    set({
      isConnected: false,
      latestBlock: null,
      latestTx: null,
      chainRevision: 0,
      reorgRevision: 0,
    }),
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
  if (
    wsInstance &&
    wsNetwork === network &&
    (wsInstance.readyState === WebSocket.CONNECTING || wsInstance.readyState === WebSocket.OPEN)
  )
    return;

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
  if (wsNetwork !== network) {
    reconnectAttempts = 0;
    useRealtimeStore.setState({
      latestBlock: null,
      latestTx: null,
      chainRevision: 0,
      reorgRevision: 0,
    });
  }
  wsNetwork = network;
  // A new socket is not live until it opens, whatever an earlier one left behind.
  useRealtimeStore.getState().setConnected(false);

  const wsUrl = wsUrlFor(network);

  try {
    wsInstance = new WebSocket(wsUrl);

    wsInstance.onopen = () => {
      useRealtimeStore.getState().setConnected(true);
      reconnectAttempts = 0;
      wsInstance?.send(JSON.stringify({ action: 'subscribe', channel: 'new_block' }));
      wsInstance?.send(JSON.stringify({ action: 'subscribe', channel: 'new_transaction' }));
      wsInstance?.send(JSON.stringify({ action: 'subscribe', channel: 'reorg' }));
    };

    wsInstance.onmessage = (event) => {
      try {
        const message = JSON.parse(event.data) as WebSocketMessage;
        if (message.type === 'new_block' || message.type === 'reorg') {
          useRealtimeStore.setState((current) => ({
            chainRevision: current.chainRevision + 1,
            reorgRevision: current.reorgRevision + (message.type === 'reorg' ? 1 : 0),
            latestBlock: message.type === 'reorg' ? null : current.latestBlock,
          }));
        }
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
  // The detached onclose will never report it, so report it here.
  useRealtimeStore.getState().setConnected(false);
}

export function useRealtimeData(enabled = true) {
  const queryClient = useQueryClient();
  const network = useActiveNetwork();
  const { isConnected, latestBlock, latestTx, setLatestBlock, setLatestTx } = useRealtimeStore();
  const handlerRef = useRef<MessageHandler | null>(null);

  const handleMessage = useCallback(
    (message: WebSocketMessage) => {
      // A new block or a reorg changes every derived fact of the pipeline snapshot (proposal
      // states, pool stages, the next-block prediction): refetch it now instead of waiting for
      // the next poll.
      if (message.type === 'new_block' || message.type === 'reorg') {
        queryClient.invalidateQueries({ queryKey: ['pipeline'] });
      }
      if (message.type === 'new_block') {
        const blockData = message.data as Block;
        setLatestBlock(blockData);

        // Only what this block itself states. `avgBlockTime` and
        // `estimatedEpochTime` are rolling window averages owned by
        // /statistics/network — overwriting them here replaced that average
        // with whatever a single block-to-block interval happened to be.
        queryClient.setQueryData(
          ['network-stats'],
          (
            old:
              | {
                  latestBlock: number;
                  syncStatus?: SyncStatus;
                  epoch?: string;
                }
              | undefined
          ) => {
            if (!old) return old;

            const epochString = `${blockData.epochNumber}(${blockData.epochIndex}/${blockData.epochLength})`;

            return {
              ...old,
              latestBlock: blockData.number,
              epoch: epochString,
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
    if (!enabled) return;
    wsSubscribers++;
    useRealtimeStore.setState({ socketHeld: true });
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
        useRealtimeStore.setState({ socketHeld: false });
      }
    };
  }, [enabled, handleMessage, network]);

  return { isConnected, latestBlock, latestTx };
}
