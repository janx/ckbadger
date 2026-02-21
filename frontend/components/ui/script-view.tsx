'use client';

import { useState } from 'react';
import Link from 'next/link';
import { Hash } from './hash';
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
      <div className={cn('text-slate-500', className)}>
        {label && <span className="mr-2 text-slate-400">{label}:</span>}
        None
      </div>
    );
  }

  const headerContent = (
    <div className="flex items-center gap-2">
      <span className="font-mono text-sm font-medium text-slate-300">{label}</span>
      {scriptInfo && (
        <Link
          href={`/scripts/${encodeURIComponent(scriptInfo.name)}`}
          onClick={(e) => e.stopPropagation()}
          className="bg-terminal-green/20 text-terminal-green hover:bg-terminal-green/30 inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs transition-colors"
        >
          {scriptInfo.name}
          {scriptInfo.scriptKind && (
            <span className="text-terminal-green/60">({scriptInfo.scriptKind})</span>
          )}
        </Link>
      )}
    </div>
  );

  return (
    <div className={cn('rounded border border-slate-800 bg-slate-900/50', className)}>
      {label && (
        <div
          className={cn(
            'flex items-center justify-between border-b border-slate-800 px-3 py-2',
            collapsible && 'cursor-pointer transition-colors hover:bg-slate-800/50'
          )}
          onClick={() => collapsible && setExpanded(!expanded)}
        >
          {headerContent}
          {collapsible && (
            <span className="text-slate-500 transition-transform">{expanded ? '▼' : '▶'}</span>
          )}
        </div>
      )}
      {expanded && (
        <div className="space-y-2 p-3 font-mono text-sm">
          <div className="flex items-start gap-2">
            <span className="w-20 shrink-0 text-slate-500">code_hash:</span>
            <Hash hash={script.codeHash} className="text-slate-300" />
          </div>
          <div className="flex items-start gap-2">
            <span className="w-20 shrink-0 text-slate-500">hash_type:</span>
            <span className="text-terminal-green rounded bg-slate-800 px-2 py-0.5 text-xs">
              {getScriptRefBadgeLabel(script.hashType)}
            </span>
          </div>
          <div className="flex items-start gap-2">
            <span className="w-20 shrink-0 text-slate-500">args:</span>
            <Hash
              hash={script.args}
              truncate={script.args.length > 66}
              startChars={20}
              endChars={20}
              className="break-all text-slate-300"
            />
          </div>
        </div>
      )}
    </div>
  );
}
