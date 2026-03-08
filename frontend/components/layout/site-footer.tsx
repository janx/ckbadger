import Link from '@/components/ui/link';
import { resolveBuildVersion } from '@/lib/runtime-config';

const quickLinks = [{ href: '/hardforks', label: 'Hardforks' }];

export function SiteFooter() {
  const buildVersion = resolveBuildVersion();

  return (
    <footer className="border-t border-slate-800/90 bg-slate-950/95">
      <div className="container mx-auto px-4 py-3">
        <div className="rounded-xl border border-slate-800/90 bg-slate-900/70 p-px font-mono text-xs text-slate-400 shadow-[0_-1px_0_rgba(148,163,184,0.04),0_18px_40px_rgba(2,6,23,0.32)]">
          <div className="flex flex-wrap gap-px rounded-[11px] bg-slate-800/80">
            <section className="min-w-0 flex-1 basis-full bg-slate-950/95 px-4 py-3 lg:basis-[32rem]">
              <p className="mb-2 text-[10px] uppercase tracking-[0.24em] text-slate-500">Credits</p>
              <p className="leading-snug text-slate-200">
                Designed by{' '}
                <a
                  href="https://x.com/busyforking"
                  target="_blank"
                  rel="noreferrer"
                  className="text-terminal-green transition-colors hover:text-green-300 hover:underline"
                >
                  @busyforking
                </a>
                , coded by Claude and Codex. <span className="text-terminal-green">❤️</span>
              </p>
            </section>

            <section className="min-w-0 flex-1 basis-[14rem] bg-slate-950/95 px-4 py-3">
              <p className="mb-2 text-[10px] uppercase tracking-[0.24em] text-slate-500">Build</p>
              <p className="break-all leading-snug text-slate-300">{buildVersion}</p>
            </section>

            <section className="min-w-0 flex-1 basis-[12rem] bg-slate-950/95 px-4 py-3">
              <p className="mb-2 text-[10px] uppercase tracking-[0.24em] text-slate-500">
                Shortcut
              </p>
              <p className="leading-snug text-slate-300">Press ? for shortcuts</p>
            </section>

            <section className="min-w-0 flex-1 basis-[12rem] bg-slate-950/95 px-4 py-3">
              <p className="mb-2 text-[10px] uppercase tracking-[0.24em] text-slate-500">Links</p>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] uppercase tracking-[0.16em]">
                {quickLinks.map((link) => (
                  <Link
                    key={link.href}
                    href={link.href}
                    className="hover:decoration-terminal-dark inline-flex items-center text-slate-400 underline decoration-slate-700/80 underline-offset-4 transition-colors hover:text-slate-200"
                  >
                    {link.label}
                  </Link>
                ))}

                <a
                  href="https://github.com/janx/ckbadger"
                  target="_blank"
                  rel="noreferrer"
                  className="hover:decoration-terminal-dark inline-flex items-center text-slate-400 underline decoration-slate-700/80 underline-offset-4 transition-colors hover:text-slate-200"
                >
                  Github
                </a>
              </div>
            </section>
          </div>
        </div>
      </div>
    </footer>
  );
}
