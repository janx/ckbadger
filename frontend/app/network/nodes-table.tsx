'use client';

import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { api, type NodeSummary } from '@/lib/api';
import { formatTimeAgo, truncateHash } from '@/lib/utils';

interface NodeFilters {
  reachable?: boolean;
  country?: string;
  version?: string;
}

// Column layout shared by the header row and every data row. Wide enough that the eight columns
// stay readable; the whole grid scrolls horizontally inside its own container on narrow screens.
const GRID_TEMPLATE = '13rem 14rem 6rem 9rem 12rem 7rem 7rem 5rem';
const GRID_MIN_WIDTH = '73rem';

const COLUMNS = ['Peer ID', 'Address', 'Version', 'Country', 'ASN', 'Status', 'Last Seen', 'RTT'];

// Empty geo/ASN columns (no MaxMind db configured) render as an honest em-dash, never a fake value.
function orDash(value: string): string {
  return value.trim().length > 0 ? value : '—';
}

function ReachableBadge({ reachable }: { reachable: boolean }) {
  return (
    <Badge variant={reachable ? 'green' : 'gray'}>{reachable ? 'Reachable' : 'Unreachable'}</Badge>
  );
}

function NodeRow({ node }: { node: NodeSummary }) {
  return (
    <TerminalRow data-testid="node-row">
      <div
        className="grid w-full items-center gap-x-4"
        style={{ gridTemplateColumns: GRID_TEMPLATE, minWidth: GRID_MIN_WIDTH }}
      >
        <span className="text-aqua truncate font-mono text-xs" title={node.peerId}>
          {truncateHash(node.peerId)}
        </span>
        <span className="text-text truncate font-mono text-xs" title={node.addr}>
          {node.addr}
        </span>
        <span className="text-text truncate font-mono text-xs">{orDash(node.version)}</span>
        <span className="text-text truncate text-xs" title={node.country}>
          {orDash(node.country)}
        </span>
        <span className="text-text-dim truncate font-mono text-xs" title={node.asn}>
          {orDash(node.asn)}
        </span>
        <span>
          <ReachableBadge reachable={node.reachable} />
        </span>
        <span className="text-text-dim text-xs">{formatTimeAgo(node.lastSeen * 1000)}</span>
        <span className="text-text-dim text-right font-mono text-xs tabular-nums">
          {node.rttMs == null ? '—' : `${node.rttMs} ms`}
        </span>
      </div>
    </TerminalRow>
  );
}

export function NodesTable() {
  const [filters, setFilters] = useState<NodeFilters>({});
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  // Accumulated rows across "Load more" pages. Reset whenever a filter changes.
  const [rows, setRows] = useState<NodeSummary[]>([]);
  const mergedTokenRef = useRef<string | null>(null);

  const { data, isLoading, isError, isFetching } = useQuery({
    queryKey: ['network', 'nodes', filters, cursor],
    queryFn: () => api.getNetworkNodes({ ...filters, cursor }),
  });

  // Merge each fetched page exactly once: the first page (cursor undefined) replaces, later pages
  // append. A per-page token guards against re-merging when react-query re-emits the same page.
  useEffect(() => {
    if (!data) return;
    const token = cursor ?? '__first__';
    if (mergedTokenRef.current === token) return;
    mergedTokenRef.current = token;
    setRows((prev) => (cursor ? [...prev, ...data.items] : data.items));
  }, [data, cursor]);

  // A filter change starts a fresh, unpaginated result set.
  function applyFilters(next: NodeFilters) {
    setFilters(next);
    setCursor(undefined);
    setRows([]);
    mergedTokenRef.current = null;
  }

  const showSkeleton = isLoading && rows.length === 0;
  const showEmpty = !isLoading && !isError && rows.length === 0;

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright font-mono text-lg font-bold">Nodes</h2>

      <TerminalPanel>
        <TerminalPanelHeader indicator={isFetching ? 'active' : 'none'}>
          Discovered Nodes
        </TerminalPanelHeader>
        <TerminalPanelContent padding="none">
          {/* Filters */}
          <div className="border-base-border flex flex-wrap items-center gap-3 border-b px-4 py-2">
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={() => applyFilters({ ...filters, reachable: undefined })}
                className={`rounded px-2 py-0.5 font-mono text-xs transition-colors ${
                  filters.reachable == null
                    ? 'bg-emphasis/15 text-emphasis'
                    : 'text-text-dim hover:text-text'
                }`}
              >
                All
              </button>
              <button
                type="button"
                onClick={() => applyFilters({ ...filters, reachable: true })}
                className={`rounded px-2 py-0.5 font-mono text-xs transition-colors ${
                  filters.reachable === true
                    ? 'bg-emphasis/15 text-emphasis'
                    : 'text-text-dim hover:text-text'
                }`}
              >
                Reachable only
              </button>
            </div>

            <input
              type="text"
              value={filters.country ?? ''}
              onChange={(e) => applyFilters({ ...filters, country: e.target.value || undefined })}
              placeholder="Country"
              aria-label="Filter by country"
              className="border-base-border bg-base-bg text-text placeholder:text-text-dim rounded border px-2 py-0.5 font-mono text-xs"
            />
            <input
              type="text"
              value={filters.version ?? ''}
              onChange={(e) => applyFilters({ ...filters, version: e.target.value || undefined })}
              placeholder="Version"
              aria-label="Filter by version"
              className="border-base-border bg-base-bg text-text placeholder:text-text-dim rounded border px-2 py-0.5 font-mono text-xs"
            />
          </div>

          {/* Table (scrolls horizontally on narrow screens) */}
          <div className="overflow-x-auto">
            <div
              className="border-base-border bg-base-surface/50 text-text-dim grid items-center gap-x-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider"
              style={{ gridTemplateColumns: GRID_TEMPLATE, minWidth: GRID_MIN_WIDTH }}
            >
              {COLUMNS.map((label, i) => (
                <div key={label} className={i === COLUMNS.length - 1 ? 'text-right' : ''}>
                  {label}
                </div>
              ))}
            </div>

            {showSkeleton ? (
              <div className="text-text-dim py-12 text-center font-mono text-sm">
                Loading nodes...
              </div>
            ) : isError ? (
              <div className="text-text-dim py-12 text-center font-mono text-sm">
                Failed to load nodes.
              </div>
            ) : showEmpty ? (
              <div className="text-text-dim py-12 text-center font-mono text-sm">
                No nodes found
              </div>
            ) : (
              rows.map((node) => <NodeRow key={node.peerId} node={node} />)
            )}
          </div>
        </TerminalPanelContent>

        {data?.nextCursor != null && (
          <TerminalPanelFooter className="flex justify-center">
            <button
              type="button"
              onClick={() => setCursor(data.nextCursor ?? undefined)}
              disabled={isFetching}
              className="text-text-dim hover:text-interactive font-mono text-xs transition-colors disabled:opacity-50"
            >
              {isFetching ? 'Loading…' : 'Load more'}
            </button>
          </TerminalPanelFooter>
        )}
      </TerminalPanel>
    </section>
  );
}
