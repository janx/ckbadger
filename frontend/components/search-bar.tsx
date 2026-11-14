'use client';

import { useState, useEffect, useLayoutEffect, useMemo, useRef, type RefObject } from 'react';
import { useRouter } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { api, type SearchResult } from '@/lib/api';
import { normalizeHash32 } from '@/lib/search-intent';
import { resolveSearchRoute } from '@/lib/search-routing';
import { cn } from '@/lib/utils';
import { cssVars, SEARCH_RESULT_COLORS } from '@/lib/design-tokens';

interface SearchBarProps {
  className?: string;
  variant?: 'default' | 'compact' | 'home';
}

/** The boards' placeholder (lib.js `header()`, U-01): lowercase, as a command is typed; no dots. */
const SEARCH_PLACEHOLDER = 'search block / tx / address / cell';
/** The placeholder of a field whose text box cannot hold the long one: 6 characters, 46.8 px. */
const SEARCH_PLACEHOLDER_SHORT = 'search';
/** JetBrains Mono's advance, 0.6 em, at the field's 13 px (`text-body`). */
const BODY_CHAR_PX = 7.8;
/**
 * The long placeholder's width in the field's font: 34 characters, 265.2 px. The field draws it
 * only when its measured text box holds all of it (PA-04 follow-up; RESPONSIVE_LAYOUT principle
 * 4, the measured width decides, not the breakpoint). In the header's field that box is the
 * input's width less 1 + 28 px on the left (the border, the prompt square's inset) and 36 + 1 px
 * on the right from md (the `/` key's room), 12 + 1 px below it: the long one needs a field
 * 331.2 px wide from md, and a phone's field (180 px at 360, capped at 288 px) never holds it.
 * At 768–1023 the header leaves the field about 169 px, at 1024 368.6 px (36vw) with one network
 * and about 281 px beside the network switcher, at 1280 460.8 px.
 */
const LONG_PLACEHOLDER_PX = SEARCH_PLACEHOLDER.length * BODY_CHAR_PX;

/** The input's horizontal insets, read from its computed style: borders and paddings. */
const INSETS = ['borderLeftWidth', 'paddingLeft', 'paddingRight', 'borderRightWidth'] as const;

/**
 * The width of the field's text box (the input's content box: its width less its borders and
 * paddings), measured before paint and kept current, as ListTable measures its container; null
 * until the field is laid out (the render before the layout effect, a hidden field, jsdom). One
 * reading, made in the layout effect and again on every resize the observer reports.
 */
function useTextBoxWidth(ref: RefObject<HTMLInputElement | null>): number | null {
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => {
      const style = window.getComputedStyle(el);
      const insets = INSETS.reduce((sum, side) => sum + parseFloat(style[side]), 0);
      const w = el.getBoundingClientRect().width - insets;
      // not laid out (no width, or no style to read) is no measurement: keep what there was
      if (w > 0) setWidth(w);
    };
    read();
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return width;
}

/**
 * Where the field's block cursor sits (Midnight, continued, move 4): the caret's index in the
 * typed text, and how far the field has scrolled, so the cursor drawn over the field lands where
 * the native caret is while the field has focus.
 */
interface CaretPosition {
  at: number;
  scrollLeft: number;
}

export function SearchBar({ className, variant = 'default' }: SearchBarProps) {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const [isInputFocused, setIsInputFocused] = useState(false);
  const [caret, setCaret] = useState<CaretPosition>({ at: 0, scrollLeft: 0 });
  const syncCaret = (el: HTMLInputElement) =>
    setCaret({ at: el.selectionStart ?? el.value.length, scrollLeft: el.scrollLeft });
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const [submitFeedback, setSubmitFeedback] = useState<string | null>(null);
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  // The short placeholder until the field is measured, so a narrow field never shows the long one.
  const textBox = useTextBoxWidth(inputRef);
  const placeholder =
    textBox !== null && textBox >= LONG_PLACEHOLDER_PX
      ? SEARCH_PLACEHOLDER
      : SEARCH_PLACEHOLDER_SHORT;

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

  const resetSearchState = () => {
    setIsOpen(false);
    setIsInputFocused(false);
    setQuery('');
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
      setSubmitFeedback('Searching…');
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

    setSubmitFeedback('No matches.');
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
  const showShortcutHints = isHome || isCompact;
  // The typed text starts after the prompt square at the boards' 9 px gap (`.search`): the
  // square's left (12 px, 16 px in the home field and from sm in the default one) + its 7 px + 9
  // = 28 px (`left-7`, `pl-7`), 32 px (`left-8`, `pl-8`). The cursor track spans the same text
  // box. The keys' room on the right is kept only where they are drawn, from md (PA-04).
  const textInset = isHome
    ? 'left-8 right-20 md:right-28'
    : isCompact
      ? 'left-7 right-3 md:right-9'
      : 'left-7 right-3 sm:left-8 sm:right-4';

  return (
    <div className={cn('relative', className)}>
      <form onSubmit={handleSearch}>
        <div
          className={cn(
            'group relative',
            isHome && 'overflow-hidden rounded-lg',
            isCompact && 'rounded'
          )}
        >
          <input
            ref={inputRef}
            data-ckbadger-global-search="true"
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(-1);
              setIsOpen(true);
              syncCaret(e.target);
            }}
            onSelect={(e) => syncCaret(e.currentTarget)}
            onScroll={(e) => syncCaret(e.currentTarget)}
            onFocus={(e) => {
              setIsOpen(true);
              setIsInputFocused(true);
              syncCaret(e.currentTarget);
            }}
            onBlur={() => setIsInputFocused(false)}
            onKeyDown={handleKeyDown}
            placeholder={placeholder}
            className={cn(
              // Midnight, continued (move 4): the field is a prompt — the cinnabar square before
              // the text (U-01; a jade ">" before it), a block cursor after it while focused (the
              // native caret is hidden under it), and
              // a jade-tinted border with a soft ring on focus. The "/" key hint stays. The
              // compact and default fields are wells (material pass v2, move 2: `.well`, dark
              // glass inset, the board's 6 px corners); the home hero field keeps its approved
              // jade border and glow. That focus look is the field's one focus line: it opts out
              // of the global jade outline (visual finish V9, one ring per control).
              'text-text-bright w-full font-mono transition-colors focus-visible:outline-none',
              isInputFocused && 'caret-transparent',
              isHome
                ? 'border-base-border border-jade/50 focus:ring-jade/25 bg-base-surface/95 placeholder:text-text-dim focus:border-jade text-body h-10 rounded-lg border pl-8 pr-20 shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.18),0_6px_20px_rgb(var(--rgb-jade)/calc(0.18*var(--m-glow)))] focus:ring-2 md:pr-28'
                : isCompact
                  ? // TX-03 (Main.dc.html): a 36 px rounded field on the surface colour, one "/" key
                    // from md; below it no key and no room for one (PA-04).
                    'well placeholder:text-text-dim focus:border-jade/50 text-body h-9 pl-7 pr-3 [--well-radius:6px] focus:shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.12),0_0_12px_rgb(var(--rgb-jade)/calc(0.1*var(--m-glow)))] md:pr-9'
                  : 'well placeholder:text-text-dim focus:border-jade/50 text-body py-2.5 pl-7 pr-3 [--well-radius:6px] focus:shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.12),0_0_12px_rgb(var(--rgb-jade)/calc(0.1*var(--m-glow)))] sm:py-3 sm:pl-8 sm:pr-4'
            )}
          />
          {/* The prompt (U-01, the boards' `.search .sq`): the 7 px cinnabar square, centred on
              the field's height, as every command line on the site begins. */}
          <span
            aria-hidden="true"
            data-testid="search-prompt"
            className={cn(
              'search-square pointer-events-none absolute top-1/2 -translate-y-1/2',
              isHome ? 'left-4' : isCompact ? 'left-3' : 'left-3 sm:left-4'
            )}
          />
          {isInputFocused && (
            <span
              aria-hidden="true"
              data-testid="search-cursor-track"
              className={cn(
                'text-body pointer-events-none absolute inset-y-0 flex items-center overflow-hidden whitespace-pre font-mono',
                textInset
              )}
            >
              <span className="invisible" style={{ marginLeft: -caret.scrollLeft }}>
                {query.slice(0, caret.at)}
              </span>
              <span className="cursor shrink-0" data-testid="search-cursor" />
            </span>
          )}
          {showShortcutHints && <SearchShortcutHints variant={isCompact ? 'compact' : 'home'} />}
          {isHome && isInputFocused && (
            <>
              <span
                data-testid="home-search-focus-glow"
                className="border-jade/55 animate-terminal-glow-pulse pointer-events-none absolute inset-0 rounded-lg border opacity-100"
              />
              <span
                data-testid="home-search-focus-border-scan"
                className="pointer-events-none absolute inset-0 overflow-hidden rounded-lg"
              >
                <span className="via-jade absolute bottom-0 left-0 h-[2px] w-24 -translate-x-full bg-gradient-to-r from-transparent to-transparent [animation:terminal-border-scan-ltr_2.4s_linear_infinite]" />
              </span>
            </>
          )}
        </div>
      </form>

      {submitFeedback && (
        <div className="text-text-dim text-small mt-1 px-1" role="status" aria-live="polite">
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
              ? 'border-jade/12 rounded-none shadow-[var(--m-drop-shadow)]'
              : 'border-base-border bg-base-surface rounded-lg shadow-lg'
          )}
          style={isCompact ? { backgroundColor: SEARCH_RESULT_COLORS.panel } : undefined}
        >
          {isLoading ? (
            <div className="text-text-dim px-4 py-3">Searching…</div>
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
                          ? 'text-jade'
                          : 'text-text hover:bg-[color:var(--row-hover)]'
                        : selectedIndex === index
                          ? 'text-jade bg-base-elevated'
                          : 'text-text hover:bg-base-elevated/50'
                    )}
                    style={
                      isCompact
                        ? selectedIndex === index
                          ? { backgroundColor: SEARCH_RESULT_COLORS.selected }
                          : cssVars({ '--row-hover': SEARCH_RESULT_COLORS.hover })
                        : undefined
                    }
                  >
                    <SearchResultIcon type={result.resultType} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{result.label}</div>
                      <div className="text-text-dim text-small truncate font-mono">{result.id}</div>
                    </div>
                    <span className="bg-base-elevated text-text-dim text-small shrink-0 rounded px-2 py-0.5 font-mono">
                      {result.resultType}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="text-text-dim px-4 py-3">No results.</div>
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
        isCompact ? 'right-2.5 hidden md:flex' : 'right-3 hidden md:flex'
      )}
    >
      <span
        data-testid="search-shortcut-key-slash"
        className={cn(
          'text-meta rounded border px-1.5 py-0.5 font-mono leading-none',
          isCompact
            ? 'border-base-border text-text-dim text-meta rounded px-1.5'
            : 'border-base-border/80 bg-base-surface/80 text-text-dim'
        )}
      >
        /
      </span>
      {!isCompact && (
        <span
          data-testid="search-shortcut-key-question"
          className="border-base-border/80 bg-base-surface/80 text-text-dim text-meta rounded border px-1.5 py-0.5 font-mono leading-none"
        >
          ?
        </span>
      )}
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
