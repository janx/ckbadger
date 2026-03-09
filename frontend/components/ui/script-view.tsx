'use client';

import { useState } from 'react';
import { AppLink } from './app-link';
import { Hash } from './hash';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { getScriptRefBadgeLabel } from '@/lib/script-ref';
import { cn } from '@/lib/utils';
import type { ScriptLookupInfo } from '@/lib/api';

interface Script {
  codeHash: string;
  hashType: string;
  args: string;
}

interface ScriptViewProps {
  script: Script | null;
  label?: string;
  className?: string;
  collapsible?: boolean;
  scriptInfo?: ScriptLookupInfo | null;
}

export function ScriptView({
  script,
  label,
  className,
  collapsible = true,
  scriptInfo,
}: ScriptViewProps) {
  const [expanded, setExpanded] = useState(!collapsible);

  if (!script) {
    return (
      <div className={cn('text-text-muted', className)}>
        {label && <span className="text-text-muted mr-2">{label}:</span>}
        None
      </div>
    );
  }

  const headerContent = (
    <div className="flex items-center gap-2">
      <span className="text-text-secondary font-mono text-sm font-medium">{label}</span>
      {scriptInfo && (
        <AppLink
          href={getScriptDetailHref({
            name: scriptInfo.name,
            codeHash: scriptInfo.codeHash,
            hashType: scriptInfo.hashType,
            scriptKind: scriptInfo.scriptKind,
          })}
          onClick={(e) => e.stopPropagation()}
          className="border-base-border bg-base-elevated/70 text-text-secondary hover:bg-base-elevated inline-flex items-center gap-1 rounded border px-2 py-0.5 text-xs transition-colors"
        >
          {scriptInfo.name}
          {scriptInfo.scriptKind && (
            <span className="text-text-muted">({scriptInfo.scriptKind})</span>
          )}
        </AppLink>
      )}
    </div>
  );

  return (
    <div className={cn('border-base-border bg-base-surface/50 rounded border', className)}>
      {label && (
        <div
          className={cn(
            'border-base-border flex items-center justify-between border-b px-3 py-2',
            collapsible && 'hover:bg-base-elevated/50 cursor-pointer transition-colors'
          )}
          onClick={() => collapsible && setExpanded(!expanded)}
        >
          {headerContent}
          {collapsible && (
            <span className="text-text-muted transition-transform">{expanded ? '▼' : '▶'}</span>
          )}
        </div>
      )}
      {expanded && (
        <div className="space-y-2 p-3 font-mono text-sm">
          <div className="flex items-start gap-2">
            <span className="text-text-muted w-20 shrink-0">code_hash:</span>
            <Hash hash={script.codeHash} className="text-text-secondary" />
          </div>
          <div className="flex items-start gap-2">
            <span className="text-text-muted w-20 shrink-0">hash_type:</span>
            <span className="border-base-border bg-base-elevated/70 text-text-secondary rounded border px-2 py-0.5 text-xs">
              {getScriptRefBadgeLabel(script.hashType)}
            </span>
          </div>
          <div className="flex items-start gap-2">
            <span className="text-text-muted w-20 shrink-0">args:</span>
            <Hash
              hash={script.args}
              truncate={script.args.length > 66}
              startChars={20}
              endChars={20}
              className="text-text-secondary break-all"
            />
          </div>
        </div>
      )}
    </div>
  );
}
