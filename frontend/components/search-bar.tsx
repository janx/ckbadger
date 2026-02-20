'use client';

import { useState, useEffect, useRef } from 'react';
import { useRouter } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api, type SearchResult } from '@/lib/api';
import { resolveSearchRoute } from '@/lib/search-routing';
import { cn } from '@/lib/utils';

interface SearchBarProps {
  className?: string;
  variant?: 'default' | 'compact';
}

export function SearchBar({ className, variant = 'default' }: SearchBarProps) {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const { data: searchResults, isLoading } = useQuery({
    queryKey: ['search', query],
    queryFn: () => api.search(query),
    enabled: query.length >= 2,
    staleTime: 30000,
  });

  const results = searchResults?.results || [];

  useEffect(() => {
    setSelectedIndex(-1);
  }, [results]);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(e.target as Node) &&
        inputRef.current &&
        !inputRef.current.contains(e.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;

    if (selectedIndex >= 0 && results[selectedIndex]) {
      router.push(results[selectedIndex].url);
      setIsOpen(false);
      setQuery('');
      return;
    }

    router.push(resolveSearchRoute(query));

    setIsOpen(false);
    setQuery('');
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.min(prev + 1, results.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.max(prev - 1, -1));
    } else if (e.key === 'Escape') {
      setIsOpen(false);
    }
  };

  const handleResultClick = (result: SearchResult) => {
    router.push(result.url);
    setIsOpen(false);
    setQuery('');
  };

  const getResultIcon = (type: string) => {
    switch (type) {
      case 'block':
        return '📦';
      case 'transaction':
        return '📄';
      case 'address':
        return '👛';
      case 'cell':
        return '🔷';
      default:
        return '🔍';
    }
  };

  const isCompact = variant === 'compact';

  return (
    <div className={cn('relative', className)}>
      <form onSubmit={handleSearch}>
        <div className="relative">
          <input
            ref={inputRef}
            data-ckbadger-global-search="true"
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIsOpen(true);
            }}
            onFocus={() => setIsOpen(true)}
            onKeyDown={handleKeyDown}
            placeholder={isCompact ? 'Search blocks, txs...' : 'Block, tx hash, address...'}
            className={cn(
              'focus:border-terminal-green focus:ring-terminal-green w-full rounded-lg border border-slate-700 bg-slate-900 font-mono text-white placeholder-slate-500 transition-colors focus:outline-none focus:ring-1',
              isCompact
                ? 'py-1.5 pl-3 pr-16 text-sm'
                : 'px-3 py-2.5 pr-20 text-sm sm:px-4 sm:py-3 sm:pr-24 sm:text-base'
            )}
          />
          <button
            type="submit"
            className={cn(
              'bg-terminal-green hover:bg-terminal-dim absolute top-1/2 -translate-y-1/2 rounded font-mono font-medium text-slate-950 transition-colors',
              isCompact
                ? 'right-1 px-2 py-0.5 text-xs'
                : 'right-1.5 px-3 py-1 text-xs sm:right-2 sm:px-4 sm:py-1.5 sm:text-sm'
            )}
          >
            Search
          </button>
        </div>
      </form>

      {isOpen && query.length >= 2 && (
        <div
          ref={dropdownRef}
          className="absolute z-50 mt-1 w-full rounded-lg border border-slate-700 bg-slate-900 shadow-lg"
        >
          {isLoading ? (
            <div className="px-4 py-3 text-slate-400">Searching...</div>
          ) : results.length > 0 ? (
            <ul className="max-h-80 overflow-auto py-1">
              {results.map((result, index) => (
                <li key={`${result.resultType}-${result.id}`}>
                  <button
                    type="button"
                    onClick={() => handleResultClick(result)}
                    className={cn(
                      'flex w-full items-center gap-3 px-4 py-2 text-left transition-colors',
                      selectedIndex === index
                        ? 'text-terminal-green bg-slate-800'
                        : 'text-slate-300 hover:bg-slate-800/50'
                    )}
                  >
                    <span className="text-lg">{getResultIcon(result.resultType)}</span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{result.label}</div>
                      <div className="truncate font-mono text-xs text-slate-500">{result.id}</div>
                    </div>
                    <span className="shrink-0 rounded bg-slate-800 px-2 py-0.5 font-mono text-xs text-slate-400">
                      {result.resultType}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="px-4 py-3 text-slate-500">No results found</div>
          )}
        </div>
      )}
    </div>
  );
}
