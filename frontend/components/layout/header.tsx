'use client';

import Link from '@/components/ui/link';
import { usePathname } from '@/src/navigation';
import { useState } from 'react';
import { CommandPalette } from '@/components/command-palette';
import { SearchBar } from '@/components/search-bar';
import { Logo } from '@/components/layout/logo';
import { GlobalStatsBar } from '@/components/stats-bar';
import { useHomeScrollStore } from '@/hooks/useHomeScrollStore';

const navLinks = [
  { href: '/dao', label: 'DAO' },
  { href: '/assets', label: 'Assets' },
  { href: '/scripts', label: 'Scripts' },
  { href: '/charts', label: 'Charts' },
];

export function Header() {
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const pathname = usePathname();
  const isHomePage = pathname === '/';
  const isLinkActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
  const heroVisible = useHomeScrollStore((s) => s.heroVisible);
  const showStatsBar = isHomePage ? !heroVisible : true;

  return (
    <header className="border-base-border bg-base-bg/95 sticky top-0 z-40 mb-4 overflow-visible border-b backdrop-blur-sm">
      <div className="container relative mx-auto flex h-[56px] items-center gap-4 px-4">
        <Logo />

        {/* Left: nav links next to logo (desktop) */}
        <nav className="relative z-10 hidden shrink-0 items-center gap-2 pl-[96px] md:flex">
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className={`rounded-md border px-3 py-1.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
                isLinkActive(link.href)
                  ? 'border-jade/40 bg-jade/8 text-jade'
                  : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
              }`}
            >
              {link.label}
            </Link>
          ))}
        </nav>

        {/* Right: search bar (desktop) */}
        <div className="hidden min-w-0 flex-1 items-center justify-end md:flex">
          <div className="w-full max-w-[clamp(18rem,36vw,36rem)]">
            <SearchBar variant={isHomePage ? 'home' : 'compact'} />
          </div>
        </div>

        <div className="ml-auto flex items-center space-x-3 md:hidden">
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
        </div>
      </div>

      <div
        className={`border-jade/10 overflow-hidden border-t bg-[#060810] transition-all duration-300 ${
          showStatsBar ? 'h-7 opacity-100' : 'h-0 border-t-0 opacity-0'
        }`}
      >
        <div className="container mx-auto flex h-7 items-center px-4 md:pl-[112px]">
          <GlobalStatsBar />
        </div>
      </div>

      {isMenuOpen && (
        <div className="border-base-border bg-base-bg/95 absolute z-50 w-full border-t shadow-xl backdrop-blur-sm md:hidden">
          <nav className="container mx-auto px-4 py-4">
            <div className="mb-4">
              <SearchBar variant={isHomePage ? 'home' : 'compact'} />
            </div>
            {navLinks.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                onClick={() => setIsMenuOpen(false)}
                className={`block rounded-md border px-3 py-2.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
                  isLinkActive(link.href)
                    ? 'border-jade/40 bg-jade/8 text-jade'
                    : 'text-text hover:text-jade hover:border-jade/20 border-transparent'
                }`}
              >
                {link.label}
              </Link>
            ))}
          </nav>
        </div>
      )}

      <CommandPalette />
    </header>
  );
}
