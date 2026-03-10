'use client';

import { useState, useEffect, useMemo, useRef } from 'react';
import { useRouter } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { api, type SearchResult } from '@/lib/api';
import { normalizeHash32 } from '@/lib/search-intent';
import { resolveSearchRoute } from '@/lib/search-routing';
import { cn } from '@/lib/utils';

interface SearchBarProps {
  className?: string;
  variant?: 'default' | 'compact' | 'home';
}

export function SearchBar({ className, variant = 'default' }: SearchBarProps) {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const [isInputFocused, setIsInputFocused] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const [submitFeedback, setSubmitFeedback] = useState<string | null>(null);
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const { data: searchResults, isLoading } = useQuery({
    queryKey: ['search', query],
    queryFn: () => api.search(query),
    enabled: query.length >= 2,
    staleTime: 30000,
  });

  const results = useMemo(() => searchResults?.results ?? [], [searchResults?.results]);

  useEffect(() => {
    setSelectedIndex(-1);
  }, [results]);

  useEffect(() => {
    setSubmitFeedback(null);
  }, [query]);

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
    const trimmed = query.trim();
    if (!trimmed) return;

    if (selectedIndex >= 0 && results[selectedIndex]) {
      router.push(results[selectedIndex].url);
      setIsOpen(false);
      setIsInputFocused(false);
      setQuery('');
      setSubmitFeedback(null);
      return;
    }

    if (isLoading) {
      setSubmitFeedback('Searching...');
      return;
    }

    const normalizedHash = normalizeHash32(trimmed);
    if (normalizedHash) {
      const blockExactHash = results.find(
        (result) => result.resultType === 'block' && result.matchKind === 'exact_hash'
      );
      if (blockExactHash) {
        router.push(blockExactHash.url);
        setIsOpen(false);
        setIsInputFocused(false);
        setQuery('');
        setSubmitFeedback(null);
        return;
      }
    }

    if (results.length === 1) {
      router.push(results[0].url);
      setIsOpen(false);
      setIsInputFocused(false);
      setQuery('');
      setSubmitFeedback(null);
      return;
    }

    if (results.length > 1) {
      setIsOpen(true);
      setSubmitFeedback('Multiple matches found. Please choose one result.');
      return;
    }

    const route = resolveSearchRoute(trimmed);
    if (trimmed.length < 2 && route) {
      router.push(route);
      setIsOpen(false);
      setIsInputFocused(false);
      setQuery('');
      setSubmitFeedback(null);
      return;
    }

    setSubmitFeedback('No matches found.');
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
      setIsInputFocused(false);
    }
  };

  const handleResultClick = (result: SearchResult) => {
    router.push(result.url);
    setIsOpen(false);
    setIsInputFocused(false);
    setQuery('');
    setSubmitFeedback(null);
  };

  const isCompact = variant === 'compact';
  const isHome = variant === 'home';

  return (
    <div className={cn('relative', className)}>
      <form onSubmit={handleSearch}>
        <div className={cn('group relative', isHome && 'overflow-hidden rounded-xl')}>
          <input
            ref={inputRef}
            data-ckbadger-global-search="true"
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(-1);
              setIsOpen(true);
            }}
            onFocus={() => {
              setIsOpen(true);
              setIsInputFocused(true);
            }}
            onBlur={() => setIsInputFocused(false)}
            onKeyDown={handleKeyDown}
            placeholder={
              isHome
                ? 'Search block / tx / address / cell ...'
                : isCompact
                  ? 'Search blocks, txs...'
                  : 'Block, tx hash, address...'
            }
            className={cn(
              'focus:border-amber focus:ring-amber border-base-border bg-base-surface placeholder-text-muted text-text-primary w-full rounded-lg border font-mono transition-colors focus:outline-none focus:ring-1',
              isHome
                ? 'border-amber/50 focus:ring-amber/25 bg-base-surface/95 placeholder:text-text-muted h-10 rounded-xl pl-4 pr-20 text-sm shadow-[0_0_0_1px_rgba(240,184,102,0.18),0_6px_20px_rgba(200,148,64,0.18)] focus:ring-2 sm:pr-28'
                : isCompact
                  ? 'py-1.5 pl-3 pr-3 text-sm'
                  : 'px-3 py-2.5 pr-3 text-sm sm:px-4 sm:py-3 sm:text-base'
            )}
          />
          {isHome && isInputFocused && (
            <>
              <span
                data-testid="home-search-focus-glow"
                className="border-amber/55 animate-terminal-glow-pulse pointer-events-none absolute inset-0 rounded-xl border opacity-100"
              />
              <span
                data-testid="home-search-focus-border-scan"
                className="pointer-events-none absolute inset-0 overflow-hidden rounded-xl"
              >
                <span className="via-amber absolute bottom-0 left-0 h-[2px] w-24 -translate-x-full bg-gradient-to-r from-transparent to-transparent [animation:terminal-border-scan-ltr_2.4s_linear_infinite]" />
              </span>
            </>
          )}
          {isHome && (
            <div className="pointer-events-none absolute inset-y-0 right-3 hidden items-center gap-1 sm:flex">
              <span className="border-base-border/80 bg-base-surface/80 text-text-muted rounded border px-1.5 py-0.5 font-mono text-[10px]">
                /
              </span>
              <span className="border-base-border/80 bg-base-surface/80 text-text-muted rounded border px-1.5 py-0.5 font-mono text-[10px]">
                ?
              </span>
            </div>
          )}
        </div>
      </form>

      {submitFeedback && (
        <div className="text-text-muted mt-1 px-1 text-xs" role="status" aria-live="polite">
          {submitFeedback}
        </div>
      )}

      {isOpen && query.trim().length >= 2 && (
        <div
          ref={dropdownRef}
          className="border-base-border bg-base-surface absolute z-50 mt-1 w-full rounded-lg border shadow-lg"
        >
          {isLoading ? (
            <div className="text-text-muted px-4 py-3">Searching...</div>
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
                        ? 'text-amber bg-base-elevated'
                        : 'text-text-secondary hover:bg-base-elevated/50'
                    )}
                  >
                    <SearchResultIcon type={result.resultType} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{result.label}</div>
                      <div className="text-text-muted truncate font-mono text-xs">{result.id}</div>
                    </div>
                    <span className="bg-base-elevated text-text-muted shrink-0 rounded px-2 py-0.5 font-mono text-xs">
                      {result.resultType}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="text-text-muted px-4 py-3">No results found</div>
          )}
        </div>
      )}
    </div>
  );
}

function SearchResultIcon({ type }: { type: string }) {
  const classes = {
    block: 'text-amber',
    transaction: 'text-warning',
    address: 'text-text-secondary',
    cell: 'text-info',
    script: 'text-info-dim',
    token: 'text-positive',
    spore: 'text-warning',
    cluster: 'text-warning-dim',
    nft: 'text-warning-dim',
    default: 'text-text-muted',
  };

  const iconColor = classes[type as keyof typeof classes] ?? classes.default;

  const icon = (() => {
    switch (type) {
      case 'block':
        return (
          <path
            d="M4.5 7.5 12 4l7.5 3.5v9L12 20l-7.5-3.5v-9ZM12 4v9m0 7v-7m7.5-5.5-7.5 3.5-7.5-3.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
      case 'transaction':
        return (
          <path
            d="M4 8h10m0 0-2.5-2.5M14 8l-2.5 2.5M20 16H10m0 0 2.5-2.5M10 16l2.5 2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
      case 'address':
        return (
          <>
            <path d="M12 12a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" />
            <path d="M6 19c.8-2 3-3.5 6-3.5s5.2 1.5 6 3.5" strokeLinecap="round" />
          </>
        );
      case 'cell':
        return (
          <path
            d="M5 9 12 5l7 4-7 4-7-4Zm0 0v6l7 4 7-4V9"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
      case 'script':
        return (
          <path
            d="M6 5h12v12H6zM9 9h6M9 12h6M9 15h4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
      case 'token':
        return (
          <path
            d="M12 6a6 6 0 1 0 0 12 6 6 0 0 0 0-12Zm0 0v12M6 12h12"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
      case 'spore':
      case 'cluster':
      case 'nft':
      case 'object':
      case 'identity':
        return (
          <path
            d="M12 5 7 8v5l5 3 5-3V8l-5-3Zm0 0v11"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
      default:
        return (
          <path
            d="m21 21-4.35-4.35M10 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Z"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        );
    }
  })();

  return (
    <span
      className={cn('inline-flex h-5 w-5 items-center justify-center', iconColor)}
      data-testid={`search-result-icon-${type}`}
      aria-hidden
    >
      <svg
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
      >
        {icon}
      </svg>
    </span>
  );
}
