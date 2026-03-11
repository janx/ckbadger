'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';

const CATEGORY_COLORS: Record<string, string> = {
  dao: '#44ee77',
  tokens: '#ff66aa',
  objects: '#bb88ff',
  other: '#666677',
};

const CATEGORY_LABELS: Record<string, string> = {
  dao: 'DAO',
  tokens: 'Tokens',
  objects: 'Objects',
  other: 'Other',
};

export function AssetEcosystem() {
  const { data, isLoading } = useQuery({
    queryKey: ['asset-ecosystem'],
    queryFn: () => api.getAssetEcosystem(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  return (
    <TerminalPanel>
      <TerminalPanelHeader>Asset Ecosystem</TerminalPanelHeader>
      <TerminalPanelContent padding="md">
        {/* Top Tokens */}
        <div className="mb-4">
          <div className="text-text-dim mb-2 text-[10px] uppercase tracking-wider">Top Tokens</div>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 3 }, (_, i) => (
                <div key={i} className="bg-base-elevated h-5 animate-pulse rounded" />
              ))}
            </div>
          ) : (
            <div className="space-y-1">
              {data?.topTokens.map((token) => (
                <div key={token.typeScriptHash} className="flex items-baseline justify-between">
                  <Link
                    href={`/tokens/${token.typeScriptHash}`}
                    className="text-text-bright hover:text-jade font-mono text-xs transition-colors"
                  >
                    {token.name ?? token.symbol ?? token.typeScriptHash.slice(0, 10)}
                  </Link>
                  <span className="text-text-dim font-mono text-xs tabular-nums">
                    {token.holdersCount.toLocaleString()} holders
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Capacity Breakdown */}
        <div>
          <div className="text-text-dim mb-2 text-[10px] uppercase tracking-wider">
            Capacity Breakdown
          </div>
          {isLoading ? (
            <div className="bg-base-elevated h-3 w-full animate-pulse rounded-full" />
          ) : (
            <>
              <div className="flex h-3 w-full overflow-hidden rounded-full">
                {data?.capacityBreakdown.map((cat) => (
                  <div
                    key={cat.category}
                    style={{
                      width: `${Math.max(parseFloat(cat.percentage), 1)}%`,
                      backgroundColor: CATEGORY_COLORS[cat.category] ?? '#666',
                    }}
                    title={`${CATEGORY_LABELS[cat.category] ?? cat.category}: ${parseFloat(cat.percentage).toFixed(1)}%`}
                  />
                ))}
              </div>

              {/* Legend */}
              <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1">
                {data?.capacityBreakdown.map((cat) => (
                  <span key={cat.category} className="flex items-center gap-1.5">
                    <span
                      className="inline-block h-2 w-2 rounded-full"
                      style={{ backgroundColor: CATEGORY_COLORS[cat.category] ?? '#666' }}
                    />
                    <span className="text-text-dim font-mono text-xs">
                      {CATEGORY_LABELS[cat.category] ?? cat.category}
                    </span>
                    <span className="text-text-bright font-mono text-xs tabular-nums">
                      {parseFloat(cat.percentage).toFixed(1)}%
                    </span>
                  </span>
                ))}
              </div>
            </>
          )}
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
