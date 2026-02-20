'use client';

import Link from 'next/link';
import { useState } from 'react';
import { SearchBar } from '@/components/search-bar';
import { Logo } from '@/components/layout/logo';

const navLinks = [
  { href: '/pipeline', label: 'Pipeline' },
  { href: '/dao', label: 'DAO' },
  { href: '/assets', label: 'Assets' },
  { href: '/scripts', label: 'Scripts' },
  { href: '/charts', label: 'Charts' },
];

export function Header() {
  const [isMenuOpen, setIsMenuOpen] = useState(false);

  return (
    <header className="sticky top-0 z-40 overflow-visible border-b border-slate-800 bg-slate-900">
      <div className="container relative mx-auto flex h-16 items-center justify-between gap-4 px-4 pl-[140px] md:pl-[220px] lg:pl-[260px]">
        <Logo />

        <div className="hidden max-w-xl flex-1 md:block">
          <SearchBar variant="compact" />
        </div>

        <nav className="hidden shrink-0 items-center space-x-6 md:flex">
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className="hover:text-terminal-green font-mono text-sm uppercase tracking-wide text-slate-400 transition"
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
        <div className="absolute z-50 w-full border-t border-slate-800 bg-slate-900 shadow-xl md:hidden">
          <nav className="container mx-auto px-4 py-4">
            <div className="mb-4">
              <SearchBar variant="compact" />
            </div>
            {navLinks.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                onClick={() => setIsMenuOpen(false)}
                className="hover:text-terminal-green block py-3 font-mono text-sm uppercase tracking-wide text-slate-400 transition"
              >
                {link.label}
              </Link>
            ))}
          </nav>
        </div>
      )}
    </header>
  );
}
