'use client';

import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '@/lib/api';

/** The arrows are Discovery address-book reports, never observed p2p connections. */
export function AdvertisementGraph({ roundId }: { roundId: number }) {
  const [showAll, setShowAll] = useState(false);
  const queryClient = useQueryClient();
  const { data, isLoading, error } = useQuery({
    queryKey: ['network', 'advertisements', roundId],
    queryFn: api.getNetworkAdvertisements,
    refetchInterval: 30000,
  });
  useEffect(() => {
    if (data && data.roundId !== roundId) {
      void queryClient.invalidateQueries({ queryKey: ['network', 'summary'] });
    }
  }, [data, roundId, queryClient]);
  if (isLoading)
    return <p className="text-text-dim text-[12px]">Loading advertisement evidence…</p>;
  if (error)
    return (
      <p role="alert" className="text-negative text-[12px]">
        Advertisement evidence unavailable: {String(error)}
      </p>
    );
  if (!data || data.roundId !== roundId)
    return (
      <p role="status" className="text-text-dim text-[12px]">
        Crawler round changed; refreshing advertisement evidence…
      </p>
    );
  const visible = showAll ? data.edges : data.edges.slice(0, 24);
  return (
    <div className="min-w-0">
      <p className="text-text-dim mb-4 max-w-4xl text-[12px] leading-relaxed">
        {data.distinctPeerPairs.toLocaleString()} unique advertiser → target peer pairs across{' '}
        {data.edges.length.toLocaleString()} retained endpoint alias observations in completed round
        #{roundId}. Arrows show address-book gossip, not connections. Retention and later updates
        can remove earlier evidence; this is a subset of what the crawler heard in that round.
      </p>
      {visible.length === 0 ? (
        <p className="text-text-dim text-[12px]">No retained advertisement edges for this round.</p>
      ) : (
        <div
          role="list"
          aria-label="Directed advertiser to advertised endpoint graph"
          className="grid gap-x-8 gap-y-0 md:grid-cols-2"
        >
          {visible.map((edge) => (
            <div
              role="listitem"
              key={edge.advertiserPeerId + ':' + edge.targetPeerId + ':' + edge.alias}
              className="border-base-border-subtle grid min-w-0 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] gap-2 border-t py-2 text-[11px]"
            >
              <span title={edge.advertiserPeerId} className="text-text-bright truncate">
                {edge.advertiserPeerId}
              </span>
              <span aria-label="advertised" className="text-jade">
                →
              </span>
              <span className="min-w-0">
                <span title={edge.targetPeerId} className="text-text-bright block truncate">
                  {edge.targetPeerId}
                </span>
                <span title={edge.alias} className="text-text-dim block truncate">
                  {edge.alias}
                </span>
              </span>
            </div>
          ))}
        </div>
      )}
      {data.edges.length > 24 && (
        <button
          type="button"
          onClick={() => setShowAll((value) => !value)}
          className="text-jade mt-3 text-[11px] underline underline-offset-2"
        >
          {showAll
            ? 'Show first 24 observations'
            : `Show all ${data.edges.length.toLocaleString()} observations`}
        </button>
      )}
    </div>
  );
}
