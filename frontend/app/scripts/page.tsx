'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from 'next/link';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { PageHeader, Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, KnownScript } from '@/lib/api';

export default function ScriptsPage() {
  const pagination = useCursorPagination();
  const decoderType = undefined;
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState<string | undefined>(undefined);

  const { data, isLoading } = useQuery({
    queryKey: ['scripts', pagination.cursor, decoderType, search],
    queryFn: () => api.getScripts({ limit: 20, cursor: pagination.cursor, decoderType, search }),
  });

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    setSearch(searchInput.trim() || undefined);
    pagination.reset();
  };

  const clearSearch = () => {
    setSearchInput('');
    setSearch(undefined);
    pagination.reset();
  };

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Scripts"
          subtitle="Browse known scripts deployed on the CKB network"
          actions={
            <form onSubmit={handleSearch} className="flex gap-2">
              <div className="relative">
                <input
                  type="text"
                  value={searchInput}
                  onChange={(e) => setSearchInput(e.target.value)}
                  placeholder="Search by name or code hash..."
                  className="focus:border-terminal-dark focus:ring-terminal-dark w-64 rounded border border-slate-700 bg-slate-900 px-3 py-1.5 font-mono text-sm text-white placeholder-slate-600 transition-colors focus:outline-none focus:ring-1"
                />
                {search && (
                  <button
                    type="button"
                    onClick={clearSearch}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300"
                  >
                    ×
                  </button>
                )}
              </div>
              <button
                type="submit"
                className="border-terminal-dark bg-terminal-dark/20 text-terminal-green hover:bg-terminal-dark/40 rounded border px-4 py-1.5 font-mono text-sm transition-colors"
              >
                Search
              </button>
            </form>
          }
        />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Script List</TerminalPanelHeader>
          <TerminalPanelContent padding="none">
            {isLoading ? (
              <div className="space-y-2 py-4">
                {Array.from({ length: 5 }).map((_, i) => (
                  <TerminalRow key={i} hoverable={false}>
                    <div className="flex animate-pulse items-center">
                      <div className="flex-1">
                        <div className="h-4 w-32 rounded bg-slate-800" />
                      </div>
                      <div className="w-20">
                        <div className="h-4 w-12 rounded bg-slate-800" />
                      </div>
                      <div className="flex-1">
                        <div className="h-4 w-48 rounded bg-slate-800" />
                      </div>
                      <div className="w-48">
                        <div className="ml-auto h-4 w-32 rounded bg-slate-800" />
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </div>
            ) : data?.data?.length ? (
              <>
                <div className="flex border-b border-slate-800 bg-slate-900/50 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
                  <div className="w-36">Script</div>
                  <div className="w-28">Status</div>
                  <div className="w-16">Kind</div>
                  <div className="flex-1 px-4">Description</div>
                  <div className="w-44 text-right">Code Hash</div>
                </div>
                {data.data.map((script: KnownScript) => (
                  <TerminalRow key={script.name}>
                    <div className="flex items-center">
                      <div className="w-36">
                        <Link
                          href={`/scripts/${encodeURIComponent(script.name)}`}
                          className="text-terminal-green font-medium hover:underline"
                        >
                          {script.name}
                        </Link>
                      </div>
                      <div className="flex w-28 gap-1">
                        {script.isSystem && <Badge variant="gray">System</Badge>}
                        {script.deprecated && <Badge variant="red">Deprecated</Badge>}
                        {!script.isSystem && !script.deprecated && (
                          <span className="text-slate-600">-</span>
                        )}
                      </div>
                      <div className="w-16">
                        {script.scriptKind ? (
                          <Badge variant={script.scriptKind === 'lock' ? 'blue' : 'purple'}>
                            {script.scriptKind}
                          </Badge>
                        ) : (
                          <span className="text-slate-600">-</span>
                        )}
                      </div>
                      <div className="flex-1 truncate px-4 text-sm text-slate-400">
                        {script.description}
                      </div>
                      <div className="w-44 text-right">
                        <HexDisplay
                          value={script.codeHash}
                          color="white"
                          size="sm"
                          startChars={8}
                          endChars={6}
                        />
                      </div>
                    </div>
                  </TerminalRow>
                ))}
              </>
            ) : (
              <div className="py-8 text-center text-slate-500">No scripts found</div>
            )}
          </TerminalPanelContent>

          {data && data.data?.length > 0 && (
            <TerminalPanelFooter>
              <CursorPagination
                total={data.total ?? undefined}
                totalLabel="scripts"
                pageSize={20}
                page={pagination.page}
                hasMore={data.hasMore}
                hasPrevious={pagination.hasPrevious}
                onNext={() => pagination.goToNext(data.nextCursor)}
                onPrevious={pagination.goToPrevious}
              />
            </TerminalPanelFooter>
          )}
        </TerminalPanel>
      </main>
    </div>
  );
}
