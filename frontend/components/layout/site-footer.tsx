'use client';

import Link from '@/components/ui/link';
import { resolveBuildVersion } from '@/lib/runtime-config';

export function SiteFooter() {
  const buildVersion = resolveBuildVersion();

  return (
    <footer className="border-base-border bg-base-void/95 border-t">
      <div className="container mx-auto px-4 py-3">
        <div className="font-mono text-[11px] leading-relaxed">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex items-center gap-3">
              <span className="text-text-ghost select-none">&gt;</span>
              <span className="text-text">
                Designed by{' '}
                <a
                  href="https://x.com/busyforking"
                  target="_blank"
                  rel="noreferrer"
                  className="text-jade hover:text-jade-dim transition-colors"
                >
                  @busyforking
                </a>
                , coded by <span className="text-aqua">Claude</span>
                {' and '}
                <span className="text-aqua">Codex</span>. ❤️✌️
              </span>
            </div>

            <div className="flex flex-wrap items-center gap-3">
              <span className="text-text-ghost hidden sm:inline">|</span>
              <a
                href="https://github.com/janx/ckbadger"
                target="_blank"
                rel="noreferrer"
                className="text-text hover:text-jade transition-colors"
              >
                {buildVersion}
              </a>
              <span className="live-dot" />
              <span className="text-text-ghost">|</span>
              <Link
                href="/hardforks"
                className="text-text hover:text-jade border-base-border hover:border-jade/30 rounded border px-1.5 py-0.5 transition-colors"
              >
                Hardforks
              </Link>
              <a
                href="https://dashboard.fiber.channel/"
                target="_blank"
                rel="noreferrer"
                className="text-text hover:text-jade border-base-border hover:border-jade/30 rounded border px-1.5 py-0.5 transition-colors"
              >
                Fiber Dashboard
              </a>
              <a
                href="https://web5.tech"
                target="_blank"
                rel="noreferrer"
                className="text-text hover:text-jade border-base-border hover:border-jade/30 rounded border px-1.5 py-0.5 transition-colors"
              >
                Web5
              </a>
              <span className="text-text-ghost">|</span>
              <span className="text-text">
                <span className="text-text-dim">?</span> keys
              </span>
            </div>
          </div>
        </div>
      </div>
    </footer>
  );
}
