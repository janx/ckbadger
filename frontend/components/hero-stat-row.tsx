'use client';

import Link from '@/components/ui/link';
import type { NetworkStats } from '@/lib/api';
import { formatScaledInteger } from '@/lib/exact-integer';
import { formatBlockNumber } from '@/lib/format-block';

interface HeroStatRowProps {
  stats: NetworkStats | null;
}

function formatKnowledgeSize(shannons: string): string {
  const amount = BigInt(shannons);
  if (amount % BigInt(100000000) !== BigInt(0)) {
    throw new Error(`knowledge size ${shannons} is not a whole occupied byte`);
  }
  const ckb = amount / BigInt(100000000);
  // 1 CKB = 1 byte of storage
  const bytes = ckb;
  if (bytes >= BigInt('1000000000000')) {
    return `${formatScaledInteger(bytes, BigInt('1000000000000'), 1)} TB`;
  }
  if (bytes >= BigInt('1000000000')) {
    return `${formatScaledInteger(bytes, BigInt('1000000000'), 1)} GB`;
  }
  if (bytes >= BigInt('1000000')) {
    return `${formatScaledInteger(bytes, BigInt('1000000'), 1)} MB`;
  }
  return `${bytes.toLocaleString()} B`;
}

function formatCkbAmount(shannons: string): string {
  const amount = BigInt(shannons);
  if (amount >= BigInt('100000000000000000')) {
    return `${formatScaledInteger(amount, BigInt('100000000000000000'), 2)}B CKB`;
  }
  if (amount >= BigInt('100000000000000')) {
    return `${formatScaledInteger(amount, BigInt('100000000000000'), 2)}M CKB`;
  }
  return `${formatScaledInteger(amount, BigInt(100000000), 4)} CKB`;
}

function formatBlockHeight(block: number): string {
  return `${formatBlockNumber(block)}`;
}

function extractEpochNumber(epoch: string): string {
  // epoch format: "8234(150/1800)" or "8234 (150/1800)" — extract the leading number
  const match = epoch.match(/^(\d+)/);
  if (!match) return epoch;
  return `#${BigInt(match[1]).toLocaleString()}`;
}

interface StatItemProps {
  value: string;
  label: string;
  href: string;
}

function StatItem({ value, label, href }: StatItemProps) {
  return (
    <Link href={href} className="group flex flex-col items-center text-center">
      <span className="text-emphasis group-hover:text-accent font-mono text-xl font-bold tabular-nums transition-colors">
        {value}
      </span>
      <span className="text-text-dim text-xs uppercase tracking-wider">{label}</span>
    </Link>
  );
}

function StatSkeleton() {
  return (
    <div className="flex flex-col items-center text-center">
      <div className="bg-base-elevated h-7 w-28 animate-pulse rounded" />
      <div className="bg-base-elevated mt-1.5 h-3 w-20 animate-pulse rounded" />
    </div>
  );
}

export function HeroStatRow({ stats }: HeroStatRowProps) {
  if (!stats) {
    return (
      <div className="flex flex-wrap items-start justify-between gap-4">
        <StatSkeleton />
        <StatSkeleton />
        <StatSkeleton />
        <StatSkeleton />
        <StatSkeleton />
      </div>
    );
  }

  return (
    <div className="flex flex-wrap items-start justify-between gap-4">
      <StatItem
        value={stats.knowledgeSize ? formatKnowledgeSize(stats.knowledgeSize) : '—'}
        label="Knowledge Size"
        href="/charts/knowledge-size"
      />
      <StatItem
        value={stats.circulatingSupply ? formatCkbAmount(stats.circulatingSupply) : '—'}
        label="Circulating"
        href="/charts/total-supply"
      />
      <StatItem
        value={stats.daoLocked ? formatCkbAmount(stats.daoLocked) : '—'}
        label="DAO Locked"
        href="/nervos-dao"
      />
      <StatItem
        value={formatBlockHeight(stats.latestBlock)}
        label="Block Height"
        href={`/blocks/${stats.latestBlock}`}
      />
      <StatItem
        value={extractEpochNumber(stats.epoch)}
        label="Epoch"
        href="/charts/epoch-time-length"
      />
    </div>
  );
}
