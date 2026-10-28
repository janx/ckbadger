'use client';

import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { create } from 'zustand';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { useRealtimeData, useRealtimeStore } from '@/hooks/useRealtimeStore';
import { api, type VesselSnapshot } from '@/lib/api';
import { compareVesselSnapshots, type VesselChange } from '@/lib/error-vessels';

interface CaptionState {
  network: string | null;
  state: 'loading' | 'live' | 'waiting' | 'stale' | 'error' | 'paused';
  anchorBlock: number | null;
  visible: number;
  blocksInWindow: number;
  message: string | null;
}

export const useVesselCaption = create<CaptionState>(() => ({
  network: null,
  state: 'loading',
  anchorBlock: null,
  visible: 0,
  blocksInWindow: 0,
  message: null,
}));

export function useErrorVessels(active: boolean) {
  const network = useActiveNetwork();
  const { isConnected, latestBlock } = useRealtimeData(active);
  const chainRevision = useRealtimeStore((state) => state.chainRevision);
  const reorgRevision = useRealtimeStore((state) => state.reorgRevision);
  const [snapshot, setSnapshot] = useState<VesselSnapshot | null>(null);
  const [snapshotNetwork, setSnapshotNetwork] = useState<string | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [change, setChange] = useState<VesselChange | null>(null);
  const previous = useRef<VesselSnapshot | null>(null);
  const requestId = useRef(0);
  const acceptedReorgRevision = useRef(0);
  const lastSignal = useRef({ network, chainRevision, isConnected });

  useEffect(() => {
    previous.current = null;
    acceptedReorgRevision.current = 0;
    setSnapshot(null);
    setSnapshotNetwork(null);
    setLocalError(null);
    setChange(null);
    useVesselCaption.setState({
      network,
      state: 'loading',
      anchorBlock: null,
      visible: 0,
      blocksInWindow: 0,
      message: null,
    });
    return () => {
      if (useVesselCaption.getState().network === network) {
        useVesselCaption.setState({
          network: null,
          state: 'loading',
          anchorBlock: null,
          visible: 0,
          blocksInWindow: 0,
          message: null,
        });
      }
    };
  }, [network]);

  const currentSnapshot = snapshotNetwork === network ? snapshot : null;
  const waiting =
    !!currentSnapshot &&
    ((isConnected &&
      !!latestBlock &&
      (currentSnapshot.anchorBlock < latestBlock.number ||
        (currentSnapshot.anchorBlock === latestBlock.number &&
          currentSnapshot.anchorHash !== latestBlock.hash))) ||
      reorgRevision > acceptedReorgRevision.current);
  const query = useQuery({
    queryKey: ['error-vessels', network],
    enabled: active,
    queryFn: async () => {
      const id = ++requestId.current;
      const result = await api.getVesselSnapshot(
        previous.current?.cells.map((cell) => cell.outpoint) ?? []
      );
      return { id, result, reorgRevision };
    },
    retry: false,
    staleTime: 0,
    gcTime: 0,
    refetchInterval: waiting ? 2000 : false,
  });

  useEffect(() => {
    const old = lastSignal.current;
    lastSignal.current = { network, chainRevision, isConnected };
    if (!active || old.network !== network) return;
    if (old.chainRevision !== chainRevision || (isConnected && !old.isConnected)) {
      void query.refetch();
    }
  }, [active, chainRevision, isConnected, network, query]);

  useEffect(() => {
    const response = query.data;
    if (!response || response.id !== requestId.current || !active) return;
    const next = response.result;
    if (
      previous.current &&
      response.reorgRevision > acceptedReorgRevision.current &&
      next.anchorBlock === previous.current.anchorBlock &&
      next.anchorHash === previous.current.anchorHash
    )
      return;
    // A lagging secondary can answer behind the last accepted anchor. Keep the current
    // sample until the same canonical chain catches up; a reorg may change the hash at
    // the same height, and is accepted as a new view.
    if (
      previous.current &&
      next.anchorBlock < previous.current.anchorBlock &&
      response.reorgRevision <= acceptedReorgRevision.current
    )
      return;
    let delta: VesselChange;
    try {
      delta = compareVesselSnapshots(previous.current, next);
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : String(error));
      setSnapshot(null);
      return;
    }
    previous.current = next;
    acceptedReorgRevision.current = response.reorgRevision;
    setLocalError(null);
    setChange(delta);
    setSnapshot(next);
    setSnapshotNetwork(network);
  }, [query.data, active, network]);

  useEffect(() => {
    const state: CaptionState['state'] = !active
      ? 'paused'
      : query.error || localError
        ? 'error'
        : !currentSnapshot
          ? 'loading'
          : !isConnected
            ? 'stale'
            : waiting
              ? 'waiting'
              : 'live';
    useVesselCaption.setState({
      network,
      state,
      anchorBlock: currentSnapshot?.anchorBlock ?? null,
      visible: currentSnapshot?.cells.length ?? 0,
      blocksInWindow: currentSnapshot?.blocksInWindow ?? 0,
      message: localError ?? (query.error instanceof Error ? query.error.message : null),
    });
  }, [active, currentSnapshot, isConnected, localError, network, query.error, waiting]);

  return { snapshot: currentSnapshot, change, isConnected };
}
