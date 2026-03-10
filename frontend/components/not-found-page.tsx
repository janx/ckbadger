'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { NotFoundCellOcean } from '@/components/not-found-cell-ocean';
import { api } from '@/lib/api';
import { truncateHash } from '@/lib/utils';

function formatTipBlock(value: number | undefined): string {
  if (value === undefined) {
    return '--';
  }
  return `#${value.toLocaleString()}`;
}

function formatTipHash(value: string | undefined): string {
  if (!value) {
    return '--';
  }
  return truncateHash(value);
}

function formatHashRate(value: string | undefined): string {
  return value ?? '--';
}

export function NotFoundPage() {
  const oceanConfig = {
    cellCount: 8,
    splitPulse: 1.2,
    haloBloom: 1.2,
    motionSpeed: 1,
  } as const;

  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    refetchInterval: 10000,
    retry: false,
  });

  const { data: blocks } = useQuery({
    queryKey: ['latest-blocks', 'not-found'],
    queryFn: () => api.getBlocks({ limit: 1 }),
    refetchInterval: 10000,
    retry: false,
  });

  const tipHash = blocks?.data[0]?.hash;

  return (
    <main className="bg-base-bg text-text-primary relative min-h-screen overflow-hidden">
      <NotFoundCellOcean
        cellCount={oceanConfig.cellCount}
        splitPulse={oceanConfig.splitPulse}
        haloBloom={oceanConfig.haloBloom}
        motionSpeed={oceanConfig.motionSpeed}
      />

      <div className="pointer-events-none absolute inset-0 bg-[linear-gradient(120deg,rgba(2,6,23,0.72)_0%,rgba(2,6,23,0.28)_38%,rgba(2,6,23,0.86)_100%)]" />

      <Header />

      <section className="relative z-10 flex min-h-[calc(100vh-4rem)] items-center justify-center px-6 py-20">
        <div className="mx-auto flex w-full max-w-4xl flex-col items-start gap-7">
          <p className="text-amber-dim font-mono text-sm tracking-[0.35em]">404</p>
          <h1 className="text-text-primary max-w-3xl font-mono text-4xl font-semibold leading-tight md:text-6xl">
            The cells you sought have fallen silent in the dark.
          </h1>
          <p className="text-text-primary/92 max-w-2xl font-mono text-lg leading-relaxed">
            Elsewhere, unborn cells are gathering light.
          </p>
          <Link
            href="/"
            className="border-amber-dim bg-amber/10 text-amber hover:bg-amber/20 rounded-md border px-5 py-2.5 font-mono text-xs uppercase tracking-[0.18em] transition"
          >
            Return Home
          </Link>
        </div>

        <div className="pointer-events-none absolute bottom-7 left-1/2 w-[min(94vw,56rem)] -translate-x-1/2">
          <div data-testid="tip-values-strip" className="bg-transparent px-5 py-2.5">
            <div className="text-text-primary/85 flex flex-wrap items-center justify-center gap-3 font-mono text-sm tabular-nums md:gap-5">
              <span>{formatTipBlock(stats?.latestBlock)}</span>
              <span className="bg-amber-dim/85 h-1 w-1 rounded-full" />
              <span>{formatTipHash(tipHash)}</span>
              <span className="bg-amber-dim/85 h-1 w-1 rounded-full" />
              <span className="text-amber">{formatHashRate(stats?.hashRate)}</span>
            </div>
          </div>
        </div>
      </section>
    </main>
  );
}
