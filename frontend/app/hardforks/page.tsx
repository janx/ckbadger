'use client';

import Link from 'next/link';
import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { Badge, PageHeader } from '@/components/ui/page-header';
import { api } from '@/lib/api';

function statusVariant(status: 'activated' | 'upcoming'): 'green' | 'amber' {
  return status === 'activated' ? 'green' : 'amber';
}

export default function HardforksPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['hardforks'],
    queryFn: () => api.getHardforks(),
    staleTime: 60_000,
  });

  const events = useMemo(
    () => [...(data?.events ?? [])].sort((a, b) => b.activationEpoch - a.activationEpoch),
    [data?.events]
  );

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="CKB Hardfork Timeline"
          subtitle={
            data
              ? `Network: ${data.network} · Tip epoch: ${data.tipEpoch.toLocaleString()} · Tip block: #${data.tipBlock.toLocaleString()}`
              : 'Protocol-level network upgrades (not chain reorg events)'
          }
        />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Protocol Upgrades</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
              <div className="w-44">Edition</div>
              <div className="w-56">Activation</div>
              <div className="w-24 text-center">Status</div>
              <div className="flex-1">Summary</div>
              <div className="w-56">Resources</div>
            </div>

            {isLoading &&
              Array.from({ length: 2 }).map((_, idx) => (
                <TerminalRow key={idx} hoverable={false}>
                  <div className="flex animate-pulse items-center">
                    <div className="h-10 w-40 rounded bg-slate-800" />
                    <div className="ml-4 h-10 w-52 rounded bg-slate-800" />
                    <div className="ml-4 h-6 w-20 rounded bg-slate-800" />
                    <div className="ml-4 h-10 flex-1 rounded bg-slate-800" />
                    <div className="ml-4 h-10 w-52 rounded bg-slate-800" />
                  </div>
                </TerminalRow>
              ))}

            {error && !isLoading && (
              <TerminalRow hoverable={false}>
                <div className="font-mono text-red-400">Failed to load hardfork timeline</div>
              </TerminalRow>
            )}

            {!isLoading &&
              !error &&
              events.map((event) => (
                <TerminalRow key={`${event.id}-${event.activationEpoch}`}>
                  <div className="flex items-center gap-4">
                    <div className="w-44">
                      <div className="font-mono text-sm text-white">{event.name}</div>
                      <div className="font-mono text-xs text-slate-500">
                        {event.editionYear} · {event.shortName}
                      </div>
                    </div>
                    <div className="w-56">
                      <div className="font-mono text-sm text-slate-300">
                        Epoch #{event.activationEpoch.toLocaleString()}
                      </div>
                      <div className="font-mono text-xs text-slate-500">
                        {event.activationDate}
                        {event.activationBlock !== null
                          ? ` · Block #${event.activationBlock.toLocaleString()}`
                          : ''}
                      </div>
                    </div>
                    <div className="w-24 text-center">
                      <Badge variant={statusVariant(event.status)}>
                        {event.status.toUpperCase()}
                      </Badge>
                    </div>
                    <div className="flex-1 pr-4 font-mono text-sm text-slate-300">
                      {event.summary}
                    </div>
                    <div className="w-56">
                      <div className="flex flex-wrap gap-x-3 gap-y-1 font-mono text-xs">
                        {event.resources.map((resource) => (
                          <a
                            key={`${event.id}-${resource.label}`}
                            href={resource.url}
                            target="_blank"
                            rel="noreferrer"
                            className="text-terminal-green hover:underline"
                          >
                            {resource.label}
                          </a>
                        ))}
                      </div>
                      {event.activationBlock !== null && (
                        <Link
                          href={`/blocks/${event.activationBlock}`}
                          className="text-terminal-green mt-1 inline-block font-mono text-xs hover:underline"
                        >
                          View activation block
                        </Link>
                      )}
                    </div>
                  </div>
                </TerminalRow>
              ))}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
