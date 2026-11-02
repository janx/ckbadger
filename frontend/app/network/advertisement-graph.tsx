'use client';

import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { truncateHash } from '@/lib/utils';

/** Five rows of two first (hierarchy R5): every observation stays one tap away. */
const FIRST_OBSERVATIONS = 10;

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
  if (isLoading) return <p className="text-text-dim text-small">Loading advertisement evidence…</p>;
  if (error)
    return (
      <p role="alert" className="text-negative text-small">
        Advertisement evidence unavailable: {String(error)}
      </p>
    );
  if (!data || data.roundId !== roundId)
    return (
      <p role="status" className="text-text-dim text-small">
        Crawler round changed; refreshing advertisement evidence…
      </p>
    );
  const visible = showAll ? data.edges : data.edges.slice(0, FIRST_OBSERVATIONS);
  return (
    <div className="min-w-0">
      <p className="text-text-dim text-body mb-4 max-w-4xl">
        {data.distinctPeerPairs.toLocaleString()} unique advertiser → target peer pairs across{' '}
        {data.edges.length.toLocaleString()} retained endpoint alias observations in completed round
        #{roundId}. Each row is one address-book report — who named whom — never a connection, so
        none of it is drawn as a link. Retention and later updates can remove earlier evidence; this
        is a subset of what the crawler heard in that round.
      </p>
      {visible.length === 0 ? (
        <p className="text-text-dim text-small">No retained advertisement edges for this round.</p>
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
              className="border-base-border-subtle text-meta grid min-w-0 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] gap-x-2 gap-y-0.5 border-t py-2"
            >
              {/* Two lines (board T3): who named whom, the ids cut in the middle so both ends
                  stay; then the address it named, wrapping whole rather than losing its port. */}
              <span
                title={edge.advertiserPeerId}
                className="text-text-bright min-w-0 [overflow-wrap:anywhere]"
              >
                {truncateHash(edge.advertiserPeerId)}
              </span>
              <span className="text-text-dim">named</span>
              <span
                title={edge.targetPeerId}
                className="text-text-bright min-w-0 [overflow-wrap:anywhere]"
              >
                {truncateHash(edge.targetPeerId)}
              </span>
              <span className="text-text-dim col-span-3 min-w-0 [overflow-wrap:anywhere]">
                {edge.alias}
              </span>
            </div>
          ))}
        </div>
      )}
      {data.edges.length > FIRST_OBSERVATIONS && (
        <button
          type="button"
          onClick={() => setShowAll((value) => !value)}
          className="text-jade text-meta mt-3 underline underline-offset-2"
        >
          {showAll
            ? `Show first ${FIRST_OBSERVATIONS} observations`
            : `Show all ${data.edges.length.toLocaleString()} observations`}
        </button>
      )}
    </div>
  );
}
