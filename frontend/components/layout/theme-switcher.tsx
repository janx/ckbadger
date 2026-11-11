'use client';

import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import { NAVBAR_MENU_PANEL_CLASS } from '@/components/layout/navbar-dropdown-styles';
import { useTheme } from '@/hooks/useTheme';
import { THEME_PREFERENCES, type ResolvedTheme, type ThemePreference } from '@/lib/theme';
import { cn } from '@/lib/utils';

export const THEME_LABELS: Record<ThemePreference, string> = {
  dark: 'Dark',
  light: 'Light',
  system: 'System',
};

/** The control's glyph: a moon at night, a sun by day, a half disc for "follow the system". */
function ThemeGlyph({
  preference,
  resolved,
  className,
}: {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  className?: string;
}) {
  const common = {
    'aria-hidden': true,
    className,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.6,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
  };
  if (preference === 'system')
    return (
      <svg {...common} data-glyph="system">
        <circle cx="12" cy="12" r="8" />
        <path d="M12 4a8 8 0 0 1 0 16Z" fill="currentColor" stroke="none" />
      </svg>
    );
  if (resolved === 'light')
    return (
      <svg {...common} data-glyph="sun">
        <circle cx="12" cy="12" r="4" />
        <path d="M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8" />
      </svg>
    );
  return (
    <svg {...common} data-glyph="moon">
      <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z" />
    </svg>
  );
}

/** What a preference row says: the name, and for System the theme it resolves to right now. */
function preferenceHint(preference: ThemePreference, system: ResolvedTheme): string | null {
  return preference === 'system' ? THEME_LABELS[system].toLowerCase() : null;
}

interface ThemeSwitcherProps {
  className?: string;
}

/**
 * The header's theme control: an icon button that opens a menu of the three preferences — Dark
 * (midnight, the default), Light (the same ink on paper), System (follow the OS). The menu
 * behaves as the header's other menus do (NavMenu): it opens on hover and on click, from the
 * keyboard as a WAI-ARIA menu button (Enter / Space / ↓ open on the first item, ↑ on the last;
 * ↑ ↓ Home End move; Escape closes back to the button); a click outside closes it. Each row is a
 * `menuitemradio` checked for the preference in force. The choice is kept in localStorage and
 * applied before the next page's first paint (lib/theme.ts, index.html).
 */
export function ThemeSwitcher({ className }: ThemeSwitcherProps) {
  const { preference, resolved, system, setPreference } = useTheme();
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [focusTarget, setFocusTarget] = useState<number | null>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const menuId = useId();

  const close = (returnFocus: boolean) => {
    setOpen(false);
    setPinned(false);
    setFocusTarget(null);
    if (returnFocus) buttonRef.current?.focus();
  };
  const openOn = (item: number) => {
    setOpen(true);
    setPinned(true);
    setFocusTarget(item);
  };

  useEffect(() => {
    if (open && focusTarget !== null) itemRefs.current[focusTarget]?.focus();
  }, [open, focusTarget]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (wrapperRef.current?.contains(event.target as Node)) return;
      setOpen(false);
      setPinned(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => document.removeEventListener('pointerdown', onPointerDown);
  }, [open]);

  const onButtonClick = (event: MouseEvent<HTMLButtonElement>) => {
    if (open && pinned) return close(false);
    // A keyboard press (Enter / Space) reaches here as a click with no pointer detail.
    if (event.detail === 0) return openOn(THEME_PREFERENCES.indexOf(preference));
    setOpen(true);
    setPinned(true);
  };
  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      openOn(event.key === 'ArrowDown' ? 0 : THEME_PREFERENCES.length - 1);
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      close(true);
    }
  };
  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = itemRefs.current.findIndex((item) => item === document.activeElement);
    const last = THEME_PREFERENCES.length - 1;
    const move = (to: number) => {
      event.preventDefault();
      itemRefs.current[to]?.focus();
    };
    if (event.key === 'ArrowDown') move(at >= last ? 0 : at + 1);
    else if (event.key === 'ArrowUp') move(at <= 0 ? last : at - 1);
    else if (event.key === 'Home') move(0);
    else if (event.key === 'End') move(last);
    else if (event.key === 'Escape') {
      event.preventDefault();
      close(true);
    }
  };
  const choose = (next: ThemePreference) => {
    setPreference(next);
    close(true);
  };

  return (
    <div
      ref={wrapperRef}
      data-testid="theme-switcher"
      className={cn('relative shrink-0', className)}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => {
        if (!pinned) setOpen(false);
      }}
      onBlur={(event) => {
        if (!wrapperRef.current?.contains(event.relatedTarget as Node | null)) close(false);
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        aria-label={`Theme: ${THEME_LABELS[preference]}`}
        title={`Theme: ${THEME_LABELS[preference]}`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-theme-preference={preference}
        onClick={onButtonClick}
        onKeyDown={onButtonKeyDown}
        className="text-text-dim hover:text-jade hover:bg-base-elevated flex h-9 w-9 items-center justify-center rounded transition-colors"
      >
        <ThemeGlyph preference={preference} resolved={resolved} className="h-[18px] w-[18px]" />
      </button>
      {open && (
        // The panel's top padding bridges the gap under the button, so the pointer stays inside.
        <div className="absolute right-0 top-full z-50 pt-1">
          <div
            id={menuId}
            role="menu"
            aria-label="Theme"
            onKeyDown={onMenuKeyDown}
            className={NAVBAR_MENU_PANEL_CLASS}
          >
            <div className="eyebrow border-base-border/70 text-text-ghost text-meta mb-1 border-b px-2.5 pb-2 pt-1 font-mono">
              Theme
            </div>
            {THEME_PREFERENCES.map((option, i) => {
              const checked = option === preference;
              const hint = preferenceHint(option, system);
              return (
                <button
                  key={option}
                  ref={(node) => {
                    itemRefs.current[i] = node;
                  }}
                  type="button"
                  role="menuitemradio"
                  aria-checked={checked}
                  tabIndex={-1}
                  onClick={() => choose(option)}
                  className={cn(
                    'text-body flex w-full items-center gap-2 whitespace-nowrap rounded-sm px-2.5 py-2 text-left font-mono transition-colors focus-visible:outline-none',
                    checked
                      ? 'bg-jade/8 text-jade'
                      : 'text-text hover:bg-base-elevated hover:text-jade focus-visible:bg-base-elevated focus-visible:text-jade'
                  )}
                >
                  <span
                    aria-hidden="true"
                    className={cn(
                      'flex h-3 w-3 shrink-0 items-center justify-center rounded-full border',
                      checked ? 'border-jade' : 'border-text-ghost'
                    )}
                  >
                    {checked && <span className="bg-jade h-1 w-1 rounded-full" />}
                  </span>
                  <span className="flex-1">{THEME_LABELS[option]}</span>
                  {hint && <span className="text-text-dim text-meta font-mono">{hint}</span>}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

interface ThemeSegmentsProps {
  className?: string;
  /** After a choice (the phone menu closes). */
  onChoose?: () => void;
}

/**
 * The phone menu's theme row: the three preferences side by side as a radio group, the one in
 * force drawn as the menu draws the current page.
 */
export function ThemeSegments({ className, onChoose }: ThemeSegmentsProps) {
  const { preference, system, setPreference } = useTheme();
  return (
    <div className={cn('px-3 py-2.5', className)} data-testid="theme-segments">
      <div className="eyebrow text-text-ghost text-meta mb-2 font-mono">Theme</div>
      <div role="radiogroup" aria-label="Theme" className="flex gap-1.5">
        {THEME_PREFERENCES.map((option) => {
          const checked = option === preference;
          const hint = preferenceHint(option, system);
          return (
            <button
              key={option}
              type="button"
              role="radio"
              aria-checked={checked}
              onClick={() => {
                setPreference(option);
                onChoose?.();
              }}
              className={cn(
                'text-small flex-1 rounded border px-3 py-2 font-mono transition-colors',
                checked
                  ? 'border-jade/40 bg-jade/8 text-jade'
                  : 'border-base-border/60 text-text-dim hover:text-jade hover:border-jade/20'
              )}
            >
              {THEME_LABELS[option]}
              {hint && <span className="text-text-ghost text-meta ml-1">({hint})</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}
