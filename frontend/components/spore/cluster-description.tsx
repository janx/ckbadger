'use client';

import { parseSporeClusterDescription } from '@/lib/spore-cluster-description';

interface ClusterDescriptionProps {
  description: string | null | undefined;
}

export function ClusterDescription({ description }: ClusterDescriptionProps) {
  const parsed = parseSporeClusterDescription(description);
  if (!parsed) {
    return null;
  }

  return (
    <div className="w-full space-y-3 text-left">
      <div className="border-base-border bg-base-surface/60 text-text-muted inline-flex items-center rounded border px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider">
        {parsed.isJson ? 'JSON description' : 'Text description'}
      </div>

      <p className="text-text-secondary whitespace-pre-wrap break-words text-sm leading-relaxed">
        {parsed.summary}
      </p>

      {parsed.metadataEntries.length > 0 && (
        <div className="grid gap-2 sm:grid-cols-2">
          {parsed.metadataEntries.map((entry) => (
            <div
              key={entry.key}
              className="border-base-border bg-base-surface/50 rounded border p-2.5"
            >
              <div className="text-text-muted font-mono text-[10px] uppercase tracking-wider">
                {entry.label}
              </div>
              <div className="text-text-primary mt-1 break-words font-mono text-xs">
                {entry.value}
              </div>
            </div>
          ))}
        </div>
      )}

      {parsed.isJson && parsed.rawJson && (
        <details className="border-base-border bg-base-bg/40 w-full overflow-hidden rounded border">
          <summary className="text-text-muted cursor-pointer px-3 py-2 text-left font-mono text-xs uppercase tracking-wider">
            View Raw Cluster Metadata JSON
          </summary>
          <pre className="border-base-border bg-base-bg/90 text-text-primary max-h-64 overflow-auto whitespace-pre-wrap break-all border-t px-3 py-2 text-left font-mono text-xs">
            {parsed.rawJson}
          </pre>
        </details>
      )}
    </div>
  );
}
