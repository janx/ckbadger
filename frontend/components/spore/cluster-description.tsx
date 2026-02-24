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
      <div className="inline-flex items-center rounded border border-slate-700 bg-slate-900/60 px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-slate-400">
        {parsed.isJson ? 'JSON description' : 'Text description'}
      </div>

      <p className="whitespace-pre-wrap break-words text-sm leading-relaxed text-slate-300">
        {parsed.summary}
      </p>

      {parsed.metadataEntries.length > 0 && (
        <div className="grid gap-2 sm:grid-cols-2">
          {parsed.metadataEntries.map((entry) => (
            <div key={entry.key} className="rounded border border-slate-800 bg-slate-900/50 p-2.5">
              <div className="font-mono text-[10px] uppercase tracking-wider text-slate-500">
                {entry.label}
              </div>
              <div className="mt-1 break-words font-mono text-xs text-slate-200">{entry.value}</div>
            </div>
          ))}
        </div>
      )}

      {parsed.isJson && parsed.rawJson && (
        <details className="w-full overflow-hidden rounded border border-slate-800 bg-slate-950/40">
          <summary className="cursor-pointer px-3 py-2 text-left font-mono text-xs uppercase tracking-wider text-slate-400">
            View Raw Cluster Metadata JSON
          </summary>
          <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all border-t border-slate-800 bg-slate-950/90 px-3 py-2 text-left font-mono text-xs text-slate-200">
            {parsed.rawJson}
          </pre>
        </details>
      )}
    </div>
  );
}
