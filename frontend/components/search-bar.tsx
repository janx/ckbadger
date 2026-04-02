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

const SEARCH_PLACEHOLDER = 'Search block / tx / address / cell ...';

export function SearchBar({ className, variant = 'default' }: SearchBarProps) {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const [isInputFocused, setIsInputFocused] = useState(false);
  const [compactCaretIndex, setCompactCaretIndex] = useState(0);
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

  const syncCompactCaretPosition = (target?: HTMLInputElement | null) => {
    if (!target) {
      return;
    }

    setCompactCaretIndex(target.selectionStart ?? target.value.length);
  };

  const resetSearchState = () => {
    setIsOpen(false);
    setIsInputFocused(false);
    setQuery('');
    setCompactCaretIndex(0);
    setSubmitFeedback(null);
  };

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) return;

    if (selectedIndex >= 0 && results[selectedIndex]) {
      router.push(results[selectedIndex].url);
      resetSearchState();
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
        resetSearchState();
        return;
      }
    }

    if (results.length === 1) {
      router.push(results[0].url);
      resetSearchState();
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
      resetSearchState();
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
    resetSearchState();
  };

  const isCompact = variant === 'compact';
  const isHome = variant === 'home';
  const compactVisibleText = query || SEARCH_PLACEHOLDER;
  const compactCaretPosition = query ? compactCaretIndex : 0;
  const compactTextBeforeCursor = compactVisibleText.slice(0, compactCaretPosition);
  const compactTextAfterCursor = compactVisibleText.slice(compactCaretPosition);
  const showShortcutHints = isHome || isCompact;

  return (
    <div className={cn('relative', className)}>
      <form onSubmit={handleSearch}>
        <div
          className={cn(
            'group relative',
            isHome && 'overflow-hidden rounded-xl',
            isCompact && 'overflow-hidden rounded-none'
          )}
        >
          {isCompact && (
            <span
              data-testid="compact-search-prompt"
              className="text-jade/70 pointer-events-none absolute inset-y-0 left-0 z-10 flex w-4 items-center justify-start font-mono text-[11px]"
            >
              &gt;
            </span>
          )}
          {isCompact && (
            <span
              data-testid="compact-search-command-line"
              aria-hidden="true"
              className="pointer-events-none absolute inset-y-0 left-4 right-14 z-10 flex items-center overflow-hidden"
            >
              <span
                data-testid="compact-search-command-text"
                className={cn(
                  'min-w-0 truncate whitespace-nowrap font-mono text-[13px] tracking-[0.015em]',
                  query ? 'text-jade/95' : 'text-text-dim/40'
                )}
              >
                <span data-testid="compact-search-text-before-cursor">
                  {compactTextBeforeCursor}
                </span>
                {isInputFocused && (
                  <span
                    data-testid="compact-search-cursor"
                    className="bg-jade/90 animate-blink-cursor inline-block h-[1.05em] w-[7px] shrink-0 align-[-0.12em]"
                  />
                )}
                <span data-testid="compact-search-text-after-cursor">{compactTextAfterCursor}</span>
              </span>
            </span>
          )}
          <input
            ref={inputRef}
            data-ckbadger-global-search="true"
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              syncCompactCaretPosition(e.target);
              setSelectedIndex(-1);
              setIsOpen(true);
            }}
            onFocus={(e) => {
              setIsOpen(true);
              setIsInputFocused(true);
              syncCompactCaretPosition(e.target);
            }}
            onBlur={() => setIsInputFocused(false)}
            onClick={(e) => syncCompactCaretPosition(e.currentTarget)}
            onKeyDown={handleKeyDown}
            onKeyUp={(e) => syncCompactCaretPosition(e.currentTarget)}
            onSelect={(e) => syncCompactCaretPosition(e.currentTarget)}
            placeholder={SEARCH_PLACEHOLDER}
            className={cn(
              'text-text-bright w-full font-mono transition-colors focus:outline-none',
              isHome
                ? 'border-base-border border-jade/50 focus:ring-jade/25 bg-base-surface/95 placeholder:text-text-dim focus:border-jade h-10 rounded-xl border pl-4 pr-20 text-sm shadow-[0_0_0_1px_rgba(46,219,163,0.18),0_6px_20px_rgba(46,219,163,0.18)] focus:ring-2 sm:pr-28'
                : isCompact
                  ? 'border-jade/18 focus:border-jade/42 h-8 rounded-none border-0 border-b bg-transparent pl-0 pr-14 text-[13px] tracking-[0.015em] text-transparent caret-transparent shadow-none placeholder:text-transparent focus:border-0 focus:border-b focus:ring-0'
                  : 'border-base-border bg-base-surface placeholder:text-text-dim focus:border-jade focus:ring-jade rounded-lg border px-3 py-2.5 pr-3 text-sm focus:ring-1 sm:px-4 sm:py-3 sm:text-base'
            )}
          />
          {showShortcutHints && <SearchShortcutHints variant={isCompact ? 'compact' : 'home'} />}
          {isHome && isInputFocused && (
            <>
              <span
                data-testid="home-search-focus-glow"
                className="border-jade/55 animate-terminal-glow-pulse pointer-events-none absolute inset-0 rounded-xl border opacity-100"
              />
              <span
                data-testid="home-search-focus-border-scan"
                className="pointer-events-none absolute inset-0 overflow-hidden rounded-xl"
              >
                <span className="via-jade absolute bottom-0 left-0 h-[2px] w-24 -translate-x-full bg-gradient-to-r from-transparent to-transparent [animation:terminal-border-scan-ltr_2.4s_linear_infinite]" />
              </span>
            </>
          )}
        </div>
      </form>

      {submitFeedback && (
        <div className="text-text-dim mt-1 px-1 text-xs" role="status" aria-live="polite">
          {submitFeedback}
        </div>
      )}

      {isOpen && query.trim().length >= 2 && (
        <div
          ref={dropdownRef}
          data-testid="search-results-dropdown"
          className={cn(
            'absolute z-50 mt-1 w-full border',
            isCompact
              ? 'border-jade/12 rounded-none bg-[#06090f] shadow-[0_14px_32px_rgba(0,0,0,0.68)]'
              : 'border-base-border bg-base-surface rounded-lg shadow-lg'
          )}
        >
          {isLoading ? (
            <div className="text-text-dim px-4 py-3">Searching...</div>
          ) : results.length > 0 ? (
            <ul className="max-h-80 overflow-auto py-1">
              {results.map((result, index) => (
                <li key={`${result.resultType}-${result.id}`}>
                  <button
                    type="button"
                    onClick={() => handleResultClick(result)}
                    className={cn(
                      'flex w-full items-center gap-3 px-4 py-2 text-left transition-colors',
                      isCompact
                        ? selectedIndex === index
                          ? 'text-jade bg-[#091217]'
                          : 'text-text hover:bg-[#070e13]'
                        : selectedIndex === index
                          ? 'text-jade bg-base-elevated'
                          : 'text-text hover:bg-base-elevated/50'
                    )}
                  >
                    <SearchResultIcon type={result.resultType} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{result.label}</div>
                      <div className="text-text-dim truncate font-mono text-xs">{result.id}</div>
                    </div>
                    <span className="bg-base-elevated text-text-dim shrink-0 rounded px-2 py-0.5 font-mono text-xs">
                      {result.resultType}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="text-text-dim px-4 py-3">No results found</div>
          )}
        </div>
      )}
    </div>
  );
}

function SearchShortcutHints({ variant }: { variant: 'compact' | 'home' }) {
  const isCompact = variant === 'compact';

  return (
    <div
      data-testid="search-shortcut-hints"
      className={cn(
        'pointer-events-none absolute inset-y-0 z-10 items-center gap-1',
        isCompact ? 'right-0 flex' : 'right-3 hidden sm:flex'
      )}
    >
      <span
        data-testid="search-shortcut-key-slash"
        className={cn(
          'rounded border px-1.5 py-0.5 font-mono text-[10px] leading-none',
          isCompact
            ? 'border-jade/14 text-text-dim/72 rounded-none bg-[#06090f]'
            : 'border-base-border/80 bg-base-surface/80 text-text-dim'
        )}
      >
        /
      </span>
      <span
        data-testid="search-shortcut-key-question"
        className={cn(
          'rounded border px-1.5 py-0.5 font-mono text-[10px] leading-none',
          isCompact
            ? 'border-jade/14 text-text-dim/72 rounded-none bg-[#06090f]'
            : 'border-base-border/80 bg-base-surface/80 text-text-dim'
        )}
      >
        ?
      </span>
    </div>
  );
}

function SearchResultIcon({ type }: { type: string }) {
  const classes = {
    block: 'text-aqua',
    transaction: 'text-jade',
    address: 'text-text-bright',
    cell: 'text-aqua-dim',
    script: 'text-lavender',
    token: 'text-gold',
    spore: 'text-lavender',
    cluster: 'text-lavender-dim',
    object: 'text-lavender-dim',
    identity: 'text-lavender-dim',
    default: 'text-text-dim',
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
