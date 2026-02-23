'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useState } from 'react';
import { CommandPalette } from '@/components/command-palette';
import { SearchBar } from '@/components/search-bar';
import { Logo } from '@/components/layout/logo';

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

  return (
    <header className="sticky top-0 z-40 overflow-visible border-b border-slate-800/80 bg-slate-950/85 backdrop-blur-md">
      <div className="container relative mx-auto flex h-16 items-center justify-between gap-4 px-4 pl-[140px] md:pl-[220px] lg:pl-[260px]">
        <Logo />

        <div className="hidden min-w-0 flex-1 items-center justify-center md:flex">
          <div className="w-full max-w-[clamp(18rem,40vw,42rem)]">
            <SearchBar variant={isHomePage ? 'home' : 'compact'} />
          </div>
        </div>

        <nav className="relative z-10 hidden shrink-0 items-center justify-end gap-2 md:flex">
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className={`rounded-md border px-3 py-1.5 font-mono text-xs uppercase tracking-[0.12em] transition ${
                isLinkActive(link.href)
                  ? 'border-terminal-green/50 bg-terminal-green/12 text-terminal-green shadow-[inset_0_0_0_1px_rgba(74,222,128,0.18)]'
                  : 'border-transparent text-slate-300/85 hover:border-slate-700/80 hover:bg-slate-800/35 hover:text-slate-100'
              }`}
            >
              {link.label}
            </Link>
          ))}
        </nav>

        <div className="flex items-center space-x-3 md:hidden">
          <button
            type="button"
            onClick={() => setIsMenuOpen(!isMenuOpen)}
            className="hover:text-terminal-green flex h-10 w-10 items-center justify-center rounded-lg text-slate-400 transition-colors hover:bg-slate-800"
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

      {isMenuOpen && (
        <div className="absolute z-50 w-full border-t border-slate-800/80 bg-slate-950/95 shadow-xl backdrop-blur-md md:hidden">
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
                    ? 'border-terminal-green/50 bg-terminal-green/12 text-terminal-green'
                    : 'border-transparent text-slate-300/85 hover:border-slate-700/80 hover:bg-slate-800/35 hover:text-slate-100'
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
