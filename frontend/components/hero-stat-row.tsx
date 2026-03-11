'use client';

import Link from '@/components/ui/link';
import type { NetworkStats } from '@/lib/api';

interface HeroStatRowProps {
  stats: NetworkStats | null;
}

function formatKnowledgeSize(shannons: string): string {
  const ckb = Number(BigInt(shannons) / BigInt(1e8));
  // 1 CKB = 1 byte of storage
  const bytes = ckb;
  if (bytes >= 1e12) {
    return `${(bytes / 1e12).toFixed(1)} TB`;
  }
  if (bytes >= 1e9) {
    return `${(bytes / 1e9).toFixed(1)} GB`;
  }
  if (bytes >= 1e6) {
    return `${(bytes / 1e6).toFixed(1)} MB`;
  }
  return `${bytes.toLocaleString()} B`;
}

function formatCkbAmount(shannons: string): string {
  const ckb = Number(BigInt(shannons) / BigInt(1e4)) / 1e4;
  if (ckb >= 1e9) {
    return `${(ckb / 1e9).toFixed(2)}B CKB`;
  }
  if (ckb >= 1e6) {
    return `${(ckb / 1e6).toFixed(2)}M CKB`;
  }
  return `${ckb.toLocaleString()} CKB`;
}

function formatBlockHeight(block: number): string {
  return `#${block.toLocaleString()}`;
}

function extractEpochNumber(epoch: string): string {
  // epoch format: "8234(150/1800)" or "8234 (150/1800)" — extract the leading number
  const match = epoch.match(/^(\d+)/);
  if (!match) return epoch;
  return `#${Number(match[1]).toLocaleString()}`;
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
