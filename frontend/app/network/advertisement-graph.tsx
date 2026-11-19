'use client';

import { useEffect, useId, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import { api } from '@/lib/api';
import { ShortId } from '@/components/ui/short-id';
import { FoldToggle } from '@/components/ui/fold-toggle';

/** Five rows of two first (hierarchy R5): every observation stays one tap away. */
const FIRST_OBSERVATIONS = 10;

/** The arrows are Discovery address-book reports, never observed p2p connections. */
export function AdvertisementGraph({ roundId }: { roundId: number }) {
  const [showAll, setShowAll] = useState(false);
  const listId = useId();
  const queryClient = useQueryClient();
  const read = useQuery({
    queryKey: ['network', 'advertisements', roundId],
    queryFn: api.getNetworkAdvertisements,
    refetchInterval: 30000,
  });
  const { data, error } = read;
  useEffect(() => {
    if (data && data.roundId !== roundId) {
      void queryClient.invalidateQueries({ queryKey: ['network', 'summary'] });
    }
  }, [data, roundId, queryClient]);
  // awaited, not isLoading: a read paused offline or between retries is still awaited
  if (isAwaited(read))
    return <p className="text-text-dim text-small">Loading advertisement evidence…</p>;
  if (error)
    return (
      <p role="alert" className="text-negative text-small max-w-measure">
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
      <p className="text-text text-body max-w-measure mb-4">
        {data.distinctPeerPairs.toLocaleString('en-US')} unique advertiser → target peer pairs
        across {data.edges.length.toLocaleString('en-US')} retained endpoint alias observations in
        completed round #{roundId}. Each row is one address-book report — who named whom — never a
        connection, so none of it is drawn as a link. Retention and later updates can remove earlier
        evidence; this is a subset of what the crawler heard in that round.
      </p>
      {visible.length === 0 ? (
        <p className="text-text text-small">No retained advertisement edges for this round.</p>
      ) : (
        <div id={listId} role="list" aria-label="Directed advertiser to advertised endpoint graph">
          {visible.map((edge) => (
            <div
              role="listitem"
              key={edge.advertiserPeerId + ':' + edge.targetPeerId + ':' + edge.alias}
              className="border-row-rule flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5 border-t py-2.5"
            >
              {/* One line (table-rows T8): who, "named", whom — the row's one bright thing — then
                  the address it named. The ids are cut in the middle so both ends stay, whole in
                  their titles; where the line is too short the address wraps under them, whole
                  rather than losing its port (board T3). */}
              <span
                title={edge.advertiserPeerId}
                className="text-text text-small whitespace-nowrap"
              >
                <ShortId value={edge.advertiserPeerId} kind="hash" size="row" />
              </span>
              <span className="text-text-dim text-meta">named</span>
              <span
                title={edge.targetPeerId}
                className="text-body text-text-bright leading-row whitespace-nowrap"
              >
                <ShortId value={edge.targetPeerId} kind="hash" size="row" />
              </span>
              <span className="text-text text-small min-w-0 [overflow-wrap:anywhere]">
                {edge.alias}
              </span>
            </div>
          ))}
        </div>
      )}
      {/* The fold both ways, in the form gate B passed for the rows' fold: FoldToggle's box, "all
          25 observations ↓" then "fewer ↑" back to the first ten, one button that keeps its
          focus (the lead's final round, item 3: the Pager's part and a second button swapped). */}
      {data.edges.length > FIRST_OBSERVATIONS && (
        <div className="mt-3">
          <FoldToggle
            open={showAll}
            count={data.edges.length}
            singular="observation"
            controls={listId}
            onToggle={setShowAll}
          />
        </div>
      )}
    </div>
  );
}
