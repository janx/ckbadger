import Link from 'next/link';

const quickLinks = [
  { href: '/blocks', label: 'Blocks' },
  { href: '/transactions', label: 'Transactions' },
  { href: '/charts', label: 'Charts' },
  { href: '/hardforks', label: 'Hardforks' },
];

export function SiteFooter() {
  return (
    <footer className="border-t border-slate-800 bg-slate-950/95">
      <div className="container mx-auto px-4 py-4">
        <div className="rounded-xl border border-slate-800/80 bg-gradient-to-br from-slate-900/80 via-slate-950 to-slate-950 px-4 py-4 font-mono text-xs text-slate-400 sm:px-5">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
            <div className="space-y-1">
              <div className="text-[11px] uppercase tracking-wider text-slate-500">
                ckbadger explorer
              </div>
              <div className="text-slate-400">
                Local-first CKB observability and protocol context
              </div>
            </div>

            <nav
              aria-label="Footer quick links"
              className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] uppercase tracking-wider"
            >
              {quickLinks.map((link) => (
                <Link
                  key={link.href}
                  href={link.href}
                  className="hover:text-terminal-green text-slate-300 transition-colors"
                >
                  {link.label}
                </Link>
              ))}
            </nav>

            <div className="inline-flex items-center rounded border border-slate-700 bg-slate-900/70 px-2 py-1 text-[11px] text-slate-500">
              Press ? for shortcuts
            </div>
          </div>

          <div className="mt-4 flex flex-col gap-2 border-t border-slate-800/80 pt-3 sm:flex-row sm:items-center sm:justify-between">
            <span className="text-slate-500">
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
            </span>
            <a
              href="https://github.com/janx/ckbadger"
              target="_blank"
              rel="noreferrer"
              className="text-terminal-green hover:underline"
            >
              Github
            </a>
          </div>
        </div>
      </div>
    </footer>
  );
}
