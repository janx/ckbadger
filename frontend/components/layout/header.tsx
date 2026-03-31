'use client';

import Link from '@/components/ui/link';
import { usePathname } from '@/src/navigation';
import { useState } from 'react';
import { CommandPalette } from '@/components/command-palette';
import { SearchBar } from '@/components/search-bar';
import { Logo } from '@/components/layout/logo';
import { GlobalStatsBar } from '@/components/stats-bar';
import { useHomeScrollStore } from '@/hooks/useHomeScrollStore';

interface NavLink {
  href: string;
  label: string;
}

interface NavDropdown {
  label: string;
  children: NavLink[];
}

type NavItem = NavLink | NavDropdown;

function isDropdown(item: NavItem): item is NavDropdown {
  return 'children' in item;
}

const navItems: NavItem[] = [
  { href: '/dao', label: 'DAO' },
  { href: '/activities', label: 'Activities' },
  {
    label: 'Inventory',
    children: [
      { href: '/inventory/tokens', label: 'Tokens' },
      { href: '/inventory/objects', label: 'Objects' },
      { href: '/inventory/identities', label: 'Identities' },
    ],
  },
  { href: '/scripts', label: 'Scripts' },
  { href: '/charts', label: 'Charts' },
];

const DESKTOP_START_COLUMN = 'hidden md:block md:w-[108px] md:shrink-0';

export function Header() {
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const pathname = usePathname();
  const isHomePage = pathname === '/';
  const isLinkActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
  const isDropdownActive = (item: NavDropdown) =>
    item.children.some((child) => isLinkActive(child.href));
  const heroVisible = useHomeScrollStore((s) => s.heroVisible);
  const showStatsBar = isHomePage ? !heroVisible : true;

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

  return (
    <header className="border-base-border bg-base-bg/95 sticky top-0 z-40 mb-4 overflow-visible border-b backdrop-blur-sm">
      <div className="container relative mx-auto flex h-[56px] items-center gap-4 px-4 md:gap-0">
        <div className="hidden md:block">
          <Logo />
        </div>
        <div
          data-testid="desktop-header-start-column"
          aria-hidden="true"
          className={DESKTOP_START_COLUMN}
        />

        <nav className="relative z-10 hidden shrink-0 items-center gap-0.5 md:flex">
          {navItems.map((item) =>
            isDropdown(item) ? (
              <div key={item.label} className="group relative">
                <button
                  type="button"
                  className={`flex items-center gap-1 rounded-md border px-2 py-1.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
                    isDropdownActive(item)
                      ? 'border-jade/40 bg-jade/8 text-jade'
                      : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                  }`}
                >
                  {item.label}
                  <svg
                    className="h-3 w-3 opacity-50"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M19 9l-7 7-7-7"
                    />
                  </svg>
                </button>
                <div className="invisible absolute left-0 top-full pt-1 opacity-0 transition-all group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100">
                  <div className="border-base-border bg-base-surface min-w-[10rem] rounded-md border py-1 shadow-lg">
                    {item.children.map((child) => (
                      <Link
                        key={child.href}
                        href={child.href}
                        className={`block px-4 py-2 font-mono text-xs uppercase tracking-[0.12em] transition-colors ${
                          isLinkActive(child.href)
                            ? 'text-jade bg-jade/8'
                            : 'text-text hover:text-jade hover:bg-base-elevated/50'
                        }`}
                      >
                        {child.label}
                      </Link>
                    ))}
                  </div>
                </div>
              </div>
            ) : (
              <Link
                key={item.href}
                href={item.href}
                className={`rounded-md border px-2 py-1.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
                  isLinkActive(item.href)
                    ? 'border-jade/40 bg-jade/8 text-jade'
                    : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                }`}
              >
                {item.label}
              </Link>
            )
          )}
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
            <span className="text-jade font-mono text-xs uppercase tracking-[0.12em]">
              {activeLabel}
            </span>
          )}
        </div>

        <div className="flex min-w-0 flex-1 items-center justify-end">
          <div className="w-full max-w-[clamp(18rem,36vw,36rem)]">
            <SearchBar variant="compact" />
          </div>
        </div>
      </div>

      <div
        className={`border-jade/10 overflow-hidden border-t bg-[#060810] transition-all duration-300 ${
          showStatsBar ? 'h-7 opacity-100' : 'h-0 border-t-0 opacity-0'
        }`}
      >
        <div className="container mx-auto flex h-7 items-center px-4">
          <div
            data-testid="desktop-stats-start-column"
            aria-hidden="true"
            className={DESKTOP_START_COLUMN}
          />
          <GlobalStatsBar />
        </div>
      </div>

      {isMenuOpen && (
        <div className="border-base-border bg-base-bg/95 absolute z-50 w-full border-t shadow-xl backdrop-blur-sm md:hidden">
          <nav className="container mx-auto px-4 py-4">
            <div className="flex flex-col gap-1">
              <Link
                href="/"
                onClick={() => setIsMenuOpen(false)}
                className={`block rounded-md border px-3 py-2.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
                  pathname === '/'
                    ? 'border-jade/40 bg-jade/8 text-jade'
                    : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                }`}
              >
                Home
              </Link>
              {navItems.map((item) =>
                isDropdown(item) ? (
                  <div key={item.label} className="flex flex-col">
                    <span
                      className={`block rounded-md border px-3 py-2.5 font-mono text-xs uppercase tracking-[0.12em] ${
                        isDropdownActive(item)
                          ? 'border-jade/40 bg-jade/8 text-jade'
                          : 'text-text border-transparent'
                      }`}
                    >
                      {item.label}
                    </span>
                    <div className="border-base-border/40 ml-3 flex flex-col gap-1 border-l pl-3 pt-1">
                      {item.children.map((child) => (
                        <Link
                          key={child.href}
                          href={child.href}
                          onClick={() => setIsMenuOpen(false)}
                          className={`block rounded-md px-3 py-2 font-mono text-[11px] uppercase tracking-[0.12em] transition ${
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
                    className={`block rounded-md border px-3 py-2.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
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
  );
}
