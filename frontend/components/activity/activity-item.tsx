'use client';

import Link from 'next/link';
import { cn, formatTimeAgo, formatCkbCompact } from '@/lib/utils';
import { HexDisplay } from '@/components/ui/hex-display';
import { TerminalRow } from '@/components/ui/terminal-panel';
import { ActivityIcon } from './activity-icon';
import { ActivityBadge } from './activity-badge';
import type { Activity, ActivityType } from '@/types/activity';

interface ActivityItemProps {
  activity: Activity;
  className?: string;
  highlighted?: boolean;
}

function getActivityLabel(activityType: ActivityType): string {
  const labels: Record<ActivityType, string> = {
    CKB_TRANSFER: 'Transferred',
    CELLBASE_REWARD: 'Mined',
    TOKEN_MINT: 'Minted',
    TOKEN_TRANSFER: 'Transferred',
    TOKEN_BURN: 'Burned',
    DOB_MINT: 'Minted',
    DOB_TRANSFER: 'Transferred',
    DOB_BURN: 'Burned',
    NFT_MINT: 'Minted',
    NFT_TRANSFER: 'Transferred',
    DAO_DEPOSIT: 'Deposited',
    DAO_WITHDRAW_REQUEST: 'Withdraw Request',
    DAO_WITHDRAW_COMPLETE: 'Withdrew',
    SCRIPT_DEPLOY: 'Deployed',
    RGBPP_TRANSFER: 'Transferred',
    RGBPP_LEAP_IN: 'Leaped In',
    RGBPP_LEAP_OUT: 'Leaped Out',
    RGBPP_ISSUANCE: 'Issued',
  };
  return labels[activityType] || 'Activity';
}

function formatAmount(activity: Activity): string | null {
  if (!activity.amount || activity.amount === '0') return null;

  const isCkbActivity =
    activity.activityCategory === 'ckb' ||
    activity.activityCategory === 'cellbase' ||
    activity.activityCategory === 'dao';

  if (isCkbActivity) {
    const { value } = formatCkbCompact(activity.amount);
    return `${value} CKB`;
  }

  const metadata = activity.metadata as {
    symbol?: string;
    decimals?: number;
    tokenName?: string;
  };

  if (metadata.decimals !== undefined) {
    const decimals = metadata.decimals;
    const rawAmount = BigInt(activity.amount);
    const divisor = BigInt(10 ** decimals);
    const whole = rawAmount / divisor;
    const fraction = rawAmount % divisor;
    const formatted =
      decimals > 0
        ? `${whole}.${fraction.toString().padStart(decimals, '0').slice(0, 4)}`
        : whole.toString();

    const symbol = metadata.symbol || metadata.tokenName || '';
    return symbol ? `${formatted} ${symbol}` : formatted;
  }

  return activity.amount;
}

export function ActivityItem({ activity, className, highlighted = false }: ActivityItemProps) {
  const label = getActivityLabel(activity.activityType);
  const amount = formatAmount(activity);

  return (
    <TerminalRow
      className={cn(
        'transition-all duration-500',
        highlighted && 'bg-amber/10 shadow-amber-glow',
        className
      )}
    >
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <div className="flex items-center gap-2">
            <ActivityIcon activityType={activity.activityType} size="md" />
            <ActivityBadge category={activity.activityCategory} />
          </div>

          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="text-sm text-slate-300">{label}</span>
              {amount && <span className="font-mono text-sm text-amber-400">{amount}</span>}
            </div>

            <div className="mt-1 flex items-center gap-2 text-xs">
              {activity.fromAddress && (
                <>
                  <span className="text-slate-600">from</span>
                  <Link href={`/address/${activity.fromAddress}`} className="hover:text-amber-400">
                    <HexDisplay
                      value={activity.fromAddress}
                      truncate
                      startChars={6}
                      endChars={4}
                      color="white"
                      size="sm"
                      showGroupHighlight={false}
                    />
                  </Link>
                </>
              )}
              {activity.fromAddress && activity.toAddress && (
                <span className="text-slate-600">→</span>
              )}
              {activity.toAddress && (
                <>
                  {!activity.fromAddress && <span className="text-slate-600">to</span>}
                  <Link href={`/address/${activity.toAddress}`} className="hover:text-amber-400">
                    <HexDisplay
                      value={activity.toAddress}
                      truncate
                      startChars={6}
                      endChars={4}
                      color="white"
                      size="sm"
                      showGroupHighlight={false}
                    />
                  </Link>
                </>
              )}
            </div>
          </div>
        </div>

        <div className="shrink-0 text-right">
          <Link href={`/tx/${activity.txHash}`} className="group block">
            <HexDisplay
              value={activity.txHash}
              truncate
              startChars={6}
              endChars={4}
              color="amber"
              size="sm"
              showGroupHighlight={false}
            />
          </Link>
          <div className="mt-1 text-xs text-slate-600">{formatTimeAgo(activity.timestamp)}</div>
        </div>
      </div>
    </TerminalRow>
  );
}
