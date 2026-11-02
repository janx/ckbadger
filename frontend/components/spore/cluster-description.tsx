'use client';

import { useRef, useState } from 'react';
import { parseSporeClusterDescription } from '@/lib/spore-cluster-description';

export function Tooltip({
  text,
  className,
  buttonClassName,
}: {
  text: string;
  className?: string;
  buttonClassName?: string;
}) {
  const [visible, setVisible] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const [pos, setPos] = useState({ top: 0, left: 0 });

  const show = () => {
    if (btnRef.current) {
      const rect = btnRef.current.getBoundingClientRect();
      setPos({ top: rect.top - 8, left: rect.left + rect.width / 2 });
    }
    setVisible(true);
  };
  const hide = () => setVisible(false);

  return (
    <span className={`inline-flex ${className ?? ''}`}>
      <button
        ref={btnRef}
        type="button"
        className={
          buttonClassName ||
          'text-text-dim hover:text-text border-base-border hover:border-text-dim text-meta ml-1 inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border font-mono leading-none transition-colors'
        }
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
        aria-label="More info"
      >
        ?
      </button>
      {visible && (
        <span
          className="bg-base-elevated border-base-border text-text text-meta pointer-events-none fixed z-[9999] w-56 -translate-x-1/2 -translate-y-full rounded border px-3 py-2 font-mono leading-relaxed shadow-lg"
          style={{ top: pos.top, left: pos.left }}
        >
          {text}
        </span>
      )}
    </span>
  );
}

interface ClusterDescriptionProps {
  description: string | null | undefined;
  /** When provided, hides the summary if it matches the cluster name (avoids duplication). */
  clusterName?: string | null;
  /** When true, DOB entries and raw JSON are hidden (handled by a separate DOB Blueprint section). */
  hideDob?: boolean;
}

export function ClusterDescription({
  description,
  clusterName,
  hideDob = false,
}: ClusterDescriptionProps) {
  const parsed = parseSporeClusterDescription(description);
  if (!parsed) {
    return null;
  }

  const summaryMatchesName =
    clusterName && parsed.summary.trim().toLowerCase() === clusterName.trim().toLowerCase();
  const showSummary = !summaryMatchesName;

  const entries = hideDob
    ? parsed.metadataEntries.filter((e) => !e.key.startsWith('dob.'))
    : parsed.metadataEntries;

  const showRawJson = parsed.isJson && parsed.rawJson && !hideDob;

  const hasContent = showSummary || entries.length > 0 || showRawJson;
  if (!hasContent) {
    return null;
  }

  return (
    <div className="w-full space-y-3 text-left">
      {showSummary && (
        <p className="text-text text-body whitespace-pre-wrap break-words leading-relaxed">
          {parsed.summary}
        </p>
      )}

      {entries.length > 0 && (
        <div className="text-body flex flex-wrap gap-x-6 gap-y-1 font-mono">
          {entries.map((entry) => (
            <div key={entry.key} className="flex items-baseline gap-2">
              <span className="text-text-dim text-small uppercase tracking-wider">
                {entry.label}
              </span>
              <span className="text-text-bright text-small tabular-nums">{entry.value}</span>
            </div>
          ))}
        </div>
      )}

      {showRawJson && (
        <details className="border-base-border bg-base-bg/40 w-full overflow-hidden rounded border">
          <summary className="text-text-dim text-small cursor-pointer px-3 py-2 text-left font-mono uppercase tracking-wider">
            View Raw Cluster Metadata JSON
          </summary>
          <pre className="border-base-border bg-base-bg/90 text-text-bright text-small max-h-64 overflow-auto whitespace-pre-wrap break-all border-t px-3 py-2 text-left font-mono">
            {parsed.rawJson}
          </pre>
        </details>
      )}
    </div>
  );
}
