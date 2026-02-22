import Link from 'next/link';

const quickLinks = [{ href: '/hardforks', label: 'Hardforks' }];

export function SiteFooter() {
  return (
    <footer className="border-t border-slate-800 bg-slate-950/95">
      <div className="container mx-auto px-4 py-1.5">
        <div className="rounded-xl bg-gradient-to-br from-slate-900/75 via-slate-950 to-slate-950 px-4 py-2 font-mono text-xs text-slate-400 sm:px-5">
          <div className="flex flex-col gap-1.5 sm:flex-row sm:items-center sm:justify-between">
            <div className="text-[11px] leading-snug text-slate-500">
              Built by{' '}
              <a
                href="https://x.com/busyforking"
                target="_blank"
                rel="noreferrer"
                className="text-terminal-green hover:underline"
              >
                @busyforking
              </a>{' '}
              with agents Coco and Dede ❤️
            </div>

            <div className="flex flex-wrap items-center gap-1.5 sm:justify-end">
              <div className="inline-flex items-center rounded border border-slate-700/80 bg-slate-900/75 px-2.5 py-1 text-[10px] text-slate-500">
                Press ? for shortcuts
              </div>

              <nav
                aria-label="Footer quick links"
                className="flex items-center gap-1.5 text-[10px] uppercase tracking-[0.16em]"
              >
                {quickLinks.map((link) => (
                  <Link
                    key={link.href}
                    href={link.href}
                    className="hover:decoration-terminal-dark inline-flex items-center px-1 text-slate-400 underline decoration-slate-700/80 underline-offset-4 transition-colors hover:text-slate-200"
                  >
                    {link.label}
                  </Link>
                ))}
              </nav>

              <a
                href="https://github.com/janx/ckbadger"
                target="_blank"
                rel="noreferrer"
                className="hover:decoration-terminal-dark inline-flex items-center px-1 text-[10px] uppercase tracking-[0.16em] text-slate-400 underline decoration-slate-700/80 underline-offset-4 transition-colors hover:text-slate-200"
              >
                Github
              </a>
            </div>
          </div>
        </div>
      </div>
    </footer>
  );
}
