'use client';

import Link from '@/components/ui/link';
import { usePathname } from '@/src/navigation';
import { useRef, useState } from 'react';
import { CommandPalette } from '@/components/command-palette';
import { SearchBar } from '@/components/search-bar';
import { Logo } from '@/components/layout/logo';
import { LOGO_NOTCH_CLASS, LOGO_STRIP_NOTCH_CLASS } from '@/components/layout/logo-geometry';
import { useStickyHeaderHeight } from '@/components/layout/sticky-header';
import { NetworkSwitcher } from '@/components/layout/network-switcher';
import { NavMenu } from '@/components/layout/nav-menu';
import {
  NAVBAR_LINK_ACTIVE_CLASS,
  NAVBAR_LINK_CLASS,
  NAVBAR_LINK_DEFAULT_CLASS,
} from '@/components/layout/navbar-dropdown-styles';
import { VitalsStrip } from '@/components/stats-bar';
import { cn } from '@/lib/utils';

interface NavLink {
  href: string;
  label: string;
}

/**
 * A section with views (TX-02, U4): on the desktop row one entry that opens a menu — the section's
 * index (`href`, named `indexLabel`) then its views; the phone menu lists the same, the section row
 * itself linking to the index.
 */
interface NavDropdown {
  href: string;
  label: string;
  indexLabel: string;
  children: NavLink[];
}

type NavItem = NavLink | NavDropdown;

function isDropdown(item: NavItem): item is NavDropdown {
  return 'children' in item;
}

const navItems: NavItem[] = [
  { href: '/dao', label: 'DAO' },
  // Review 2026-11-03, item 4 (Nav.dc.html): Activities opens as Inventory does — what happened,
  // the transactions that carry it, the blocks that order them, and the blocks that lost.
  {
    href: '/activities',
    label: 'Activities',
    indexLabel: 'Activities',
    children: [
      { href: '/transactions', label: 'Transactions' },
      { href: '/blocks', label: 'Blocks' },
      { href: '/forks', label: 'Forks' },
    ],
  },
  {
    href: '/inventory',
    label: 'Inventory',
    indexLabel: 'Inventory index',
    children: [
      { href: '/inventory/tokens', label: 'Tokens' },
      { href: '/inventory/objects', label: 'Objects' },
      { href: '/inventory/identities', label: 'Identities' },
    ],
  },
  { href: '/scripts', label: 'Scripts' },
  { href: '/charts', label: 'Charts' },
];

export function Header() {
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const headerRef = useRef<HTMLElement>(null);
  const stripRef = useRef<HTMLDivElement>(null);
  useStickyHeaderHeight(wrapperRef, headerRef, stripRef);
  const pathname = usePathname();
  const isLinkActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
  const isDropdownActive = (item: NavDropdown) =>
    isLinkActive(item.href) || item.children.some((child) => isLinkActive(child.href));

  const activeLabel = (() => {
    if (pathname === '/') return 'Home';
    for (const item of navItems) {
      if (isDropdown(item)) {
        const child = item.children.find((c) => isLinkActive(c.href));
        if (child) return child.label;
      } else if (isLinkActive(item.href)) {
        return item.label;
      }
    }
    return null;
  })();

  // From md up the header row and the vitals strip stick together (this wrapper is the sticky
  // block). Below md the wrapper is display: contents, so only the header row sticks — to the page —
  // and the strip scrolls away with the content.
  return (
    <div ref={wrapperRef} className="contents md:sticky md:top-0 md:z-40 md:mb-4 md:block">
      {/* The header's bottom edge is a tube (Midnight, continued, move 2): `.hairline-tube-header`
          paints the 1 px `border-b` jade at the left, fading into the subtle border. */}
      <header
        ref={headerRef}
        className="border-base-border-subtle hairline-tube-header bg-base-bg/95 sticky top-0 z-40 overflow-visible border-b backdrop-blur-sm md:static md:z-auto"
      >
        <div className="container relative mx-auto flex min-h-[52px] items-center gap-2 md:gap-8 lg:min-h-[60px]">
          <Logo />
          <div data-testid="header-logo-notch" aria-hidden="true" className={LOGO_NOTCH_CLASS} />

          <nav
            aria-label="Main"
            className="relative z-10 hidden shrink-0 items-center gap-x-4 md:flex lg:gap-x-6"
          >
            <NetworkSwitcher />
            {/* TX-02 (Main.dc.html): five entries; the current one bright and aria-current.
                U4: Inventory opens a menu of its index and its three views; review 2026-11-03
                item 4: so does Activities (Activities · Transactions · Blocks · Forks). */}
            {navItems.map((item) => {
              if (isDropdown(item))
                return (
                  <NavMenu
                    key={item.href}
                    label={item.label}
                    index={{ href: item.href, label: item.indexLabel }}
                    items={item.children}
                    current={isDropdownActive(item)}
                    pathname={pathname}
                  />
                );
              const current = isLinkActive(item.href);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  aria-current={current ? 'page' : undefined}
                  className={cn(
                    NAVBAR_LINK_CLASS,
                    current ? NAVBAR_LINK_ACTIVE_CLASS : NAVBAR_LINK_DEFAULT_CLASS
                  )}
                >
                  {item.label}
                </Link>
              );
            })}
          </nav>

          <div className="flex items-center gap-2 md:hidden">
            <button
              type="button"
              onClick={() => setIsMenuOpen(!isMenuOpen)}
              className="text-text-dim hover:text-interactive hover:bg-base-elevated flex h-10 w-10 items-center justify-center rounded-lg transition-colors"
              aria-label="Toggle menu"
            >
              {isMenuOpen ? (
                <svg className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                </svg>
              ) : (
                <svg className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M4 6h16M4 12h16M4 18h16"
                  />
                </svg>
              )}
            </button>
            {activeLabel && (
              <span className="text-jade text-body truncate font-mono">{activeLabel}</span>
            )}
          </div>

          <div className="flex min-w-0 flex-1 items-center justify-end gap-3">
            <div className="w-full max-w-[clamp(18rem,36vw,36rem)]">
              <SearchBar variant="compact" />
            </div>
          </div>
        </div>

        {isMenuOpen && (
          <div className="border-base-border bg-base-bg/95 absolute z-50 w-full border-t shadow-xl backdrop-blur-sm md:hidden">
            <nav className="container mx-auto py-4">
              <div className="flex flex-col gap-1">
                <Link
                  href="/"
                  onClick={() => setIsMenuOpen(false)}
                  className={`text-body block rounded-md border px-3 py-2.5 font-mono transition ${
                    pathname === '/'
                      ? 'border-jade/40 bg-jade/8 text-jade'
                      : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                  }`}
                >
                  Home
                </Link>
                <NetworkSwitcher className="mb-1 w-full" onSwitch={() => setIsMenuOpen(false)} />
                {navItems.map((item) =>
                  isDropdown(item) ? (
                    <div key={item.label} className="flex flex-col">
                      {/* U4: the section row is the index's link, as the desktop menu's first item. */}
                      <Link
                        href={item.href}
                        onClick={() => setIsMenuOpen(false)}
                        className={`text-body block rounded-md border px-3 py-2.5 font-mono transition ${
                          isDropdownActive(item)
                            ? 'border-jade/40 bg-jade/8 text-jade'
                            : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                        }`}
                      >
                        {item.label}
                      </Link>
                      <div className="border-base-border/40 ml-3 flex flex-col gap-1 border-l pl-3 pt-1">
                        {item.children.map((child) => (
                          <Link
                            key={child.href}
                            href={child.href}
                            onClick={() => setIsMenuOpen(false)}
                            className={`text-small block rounded-md px-3 py-2 font-mono transition ${
                              isLinkActive(child.href)
                                ? 'text-jade bg-jade/8'
                                : 'text-text-dim hover:text-jade'
                            }`}
                          >
                            {child.label}
                          </Link>
                        ))}
                      </div>
                    </div>
                  ) : (
                    <Link
                      key={item.href}
                      href={item.href}
                      onClick={() => setIsMenuOpen(false)}
                      className={`text-body block rounded-md border px-3 py-2.5 font-mono transition ${
                        isLinkActive(item.href)
                          ? 'border-jade/40 bg-jade/8 text-jade'
                          : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                      }`}
                    >
                      {item.label}
                    </Link>
                  )
                )}
              </div>
            </nav>
          </div>
        )}

        <CommandPalette />
      </header>
      <div
        ref={stripRef}
        className="border-base-border-subtle mb-4 border-b bg-[rgba(10,12,18,0.8)] md:mb-0"
      >
        <div className="container mx-auto flex">
          <div
            data-testid="vitals-logo-notch"
            aria-hidden="true"
            className={LOGO_STRIP_NOTCH_CLASS}
          />
          <VitalsStrip />
        </div>
      </div>
    </div>
  );
}
