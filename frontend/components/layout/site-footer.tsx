import Link from '@/components/ui/link';
import { resolveBuildVersion } from '@/lib/runtime-config';

const quickLinks = [{ href: '/hardforks', label: 'Hardforks' }];

export function SiteFooter() {
  const buildVersion = resolveBuildVersion();

  return (
    <footer className="border-base-border bg-base-bg/95 border-t">
      <div className="container mx-auto px-4 py-1.5">
        <div className="bg-base-surface text-text-dim rounded-xl px-4 py-2 font-mono text-xs sm:px-5">
          <div className="flex flex-col gap-1.5 sm:flex-row sm:items-center sm:justify-between">
            <div className="text-text-dim text-[11px] leading-snug">
              Built by{' '}
              <a
                href="https://x.com/busyforking"
                target="_blank"
                rel="noreferrer"
                className="text-jade hover:underline"
              >
                @busyforking
              </a>{' '}
              with agents Coco and Dede ❤️
            </div>

            <div className="flex flex-wrap items-center gap-1.5 sm:justify-end">
              <div className="border-base-border/80 bg-base-surface/75 text-text-dim inline-flex items-center rounded border px-2.5 py-1 text-[10px]">
                <span className="live-dot mr-1.5" />
                {buildVersion}
              </div>

              <div className="border-base-border/80 bg-base-surface/75 text-text-dim inline-flex items-center rounded border px-2.5 py-1 text-[10px]">
                Press ? for shortcuts
              </div>

              {quickLinks.map((link) => (
                <Link
                  key={link.href}
                  href={link.href}
                  className="text-text-dim decoration-base-border/80 hover:text-text-bright hover:decoration-aqua-dim inline-flex items-center px-1 underline underline-offset-4 transition-colors"
                >
                  {link.label}
                </Link>
              ))}

              <a
                href="https://github.com/janx/ckbadger"
                target="_blank"
                rel="noreferrer"
                className="text-text-dim decoration-base-border/80 hover:text-text-bright hover:decoration-aqua-dim inline-flex items-center px-1 text-[10px] uppercase tracking-[0.16em] underline underline-offset-4 transition-colors"
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
