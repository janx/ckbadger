import { cn } from '@/lib/utils';
import type { ActivityCategory } from '@/types/activity';

interface ActivityBadgeProps {
  category: ActivityCategory;
  className?: string;
}

const categoryConfig: Record<ActivityCategory, { label: string; colorClass: string }> = {
  ckb: { label: 'CKB', colorClass: 'bg-amber-500/20 text-amber-400 border-amber-500/30' },
  cellbase: {
    label: 'Cellbase',
    colorClass: 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
  },
  token: { label: 'Token', colorClass: 'bg-blue-500/20 text-blue-400 border-blue-500/30' },
  dob: { label: 'DOB', colorClass: 'bg-purple-500/20 text-purple-400 border-purple-500/30' },
  nft: { label: 'NFT', colorClass: 'bg-purple-500/20 text-purple-400 border-purple-500/30' },
  dao: { label: 'DAO', colorClass: 'bg-green-500/20 text-green-400 border-green-500/30' },
  script: { label: 'Script', colorClass: 'bg-slate-500/20 text-slate-400 border-slate-500/30' },
  rgbpp: { label: 'RGB++', colorClass: 'bg-cyan-500/20 text-cyan-400 border-cyan-500/30' },
};

export function ActivityBadge({ category, className }: ActivityBadgeProps) {
  const config = categoryConfig[category] || categoryConfig.ckb;

  return (
    <span
      className={cn(
        'inline-flex items-center rounded border px-1.5 py-0.5',
        'font-mono text-[10px] uppercase tracking-wider',
        config.colorClass,
        className
      )}
    >
      {config.label}
    </span>
  );
}
