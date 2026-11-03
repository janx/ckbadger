'use client';

import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import Link from '@/components/ui/link';
import {
  NAVBAR_LINK_ACTIVE_CLASS,
  NAVBAR_LINK_CLASS,
  NAVBAR_LINK_DEFAULT_CLASS,
  NAVBAR_MENU_ITEM_ACTIVE_CLASS,
  NAVBAR_MENU_ITEM_CLASS,
  NAVBAR_MENU_ITEM_DEFAULT_CLASS,
  NAVBAR_MENU_PANEL_CLASS,
} from '@/components/layout/navbar-dropdown-styles';
import { cn } from '@/lib/utils';

export interface NavMenuEntry {
  href: string;
  label: string;
}

/**
 * Lane U (U4): a header nav entry that opens a menu of its section's pages — the section's index
 * first (when the section has a page of its own), a hairline, then its views. It opens on hover, on click (which also keeps a hovered menu
 * open), and from the keyboard (the WAI-ARIA menu button: Enter / Space / ↓ open on the first item,
 * ↑ on the last; ↑ ↓ Home End move; Escape closes back to the button; Tab or a click outside
 * closes). Closed, the menu is not rendered. The button reads as the nav's links do: bright with
 * `aria-current` while the page is in the section; the page itself is `aria-current` in the menu.
 */
export function NavMenu({
  label,
  index,
  items,
  current,
  pathname,
}: {
  label: string;
  /**
   * The section's own page, first in the menu. Absent when the section has none of its own
   * (review 2026-11-03: `/inventory` only redirects to Tokens, so an "Inventory index" entry
   * repeated the Tokens view): the menu is then the views alone, with no hairline.
   */
  index?: NavMenuEntry;
  items: readonly NavMenuEntry[];
  /** The page is in this section. */
  current: boolean;
  pathname: string;
}) {
  const [open, setOpen] = useState(false);
  // A click (or the keyboard) pins the menu open; a hover-opened one closes when the pointer leaves.
  const [pinned, setPinned] = useState(false);
  const [focusTarget, setFocusTarget] = useState<{ index: number } | null>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLAnchorElement | null)[]>([]);
  const menuId = useId();
  const entries = index ? [index, ...items] : [...items];

  const close = (returnFocus: boolean) => {
    setOpen(false);
    setPinned(false);
    if (returnFocus) buttonRef.current?.focus();
  };
  const openOn = (item: number) => {
    setOpen(true);
    setPinned(true);
    setFocusTarget({ index: item });
  };

  useEffect(() => {
    if (open && focusTarget) itemRefs.current[focusTarget.index]?.focus();
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
    if (event.detail === 0) return openOn(0);
    setOpen(true);
    setPinned(true);
  };
  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      openOn(event.key === 'ArrowDown' ? 0 : entries.length - 1);
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      close(true);
    }
  };
  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = itemRefs.current.findIndex((item) => item === document.activeElement);
    const last = entries.length - 1;
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

  return (
    <div
      ref={wrapperRef}
      className="relative"
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
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-current={current ? 'true' : undefined}
        onClick={onButtonClick}
        onKeyDown={onButtonKeyDown}
        className={cn(
          NAVBAR_LINK_CLASS,
          'flex items-center gap-1',
          current ? NAVBAR_LINK_ACTIVE_CLASS : NAVBAR_LINK_DEFAULT_CLASS
        )}
      >
        {label}
        <svg
          aria-hidden="true"
          className={cn('h-3 w-3 opacity-50 transition-transform', open && 'rotate-180')}
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>
      {open && (
        // The panel's top padding bridges the gap under the button, so the pointer stays inside.
        <div className="absolute left-0 top-full z-50 pt-1">
          <div
            id={menuId}
            role="menu"
            aria-label={label}
            onKeyDown={onMenuKeyDown}
            className={NAVBAR_MENU_PANEL_CLASS}
          >
            {entries.map((entry, i) => {
              const here = pathname === entry.href;
              return (
                <div key={entry.href} role="none">
                  {index && i === 1 && (
                    <div role="separator" className="border-base-border/70 mx-1 my-1 border-t" />
                  )}
                  <Link
                    ref={(node) => {
                      itemRefs.current[i] = node;
                    }}
                    href={entry.href}
                    role="menuitem"
                    tabIndex={-1}
                    aria-current={here ? 'page' : undefined}
                    onClick={() => close(false)}
                    className={cn(
                      NAVBAR_MENU_ITEM_CLASS,
                      here ? NAVBAR_MENU_ITEM_ACTIVE_CLASS : NAVBAR_MENU_ITEM_DEFAULT_CLASS
                    )}
                  >
                    {entry.label}
                  </Link>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
