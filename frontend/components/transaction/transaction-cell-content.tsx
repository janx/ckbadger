'use client';

import Link from '@/components/ui/link';
import { Hash } from '@/components/ui/hash';
import { formatStandard } from '@/components/activity-event-row';
import type { TransactionCellDisplay } from '@/lib/api';
import { formatCompositionTier } from '@/lib/asset-utils';
import { BITCOIN_COLORS } from '@/lib/design-tokens';
import {
  getIdentityItemDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import { cn } from '@/lib/utils';

interface TransactionCellContentProps {
  display: TransactionCellDisplay;
  cellHref: string;
}

/** Compact cell identity; all amounts, names and media provenance come from its API descriptor. */
export function TransactionCellContent({ display, cellHref }: TransactionCellContentProps) {
  const standard =
    display.standard === 'xudt'
      ? 'xUDT'
      : display.standard === 'sudt'
        ? 'sUDT'
        : display.kind === 'file'
          ? 'CKBFS file'
          : formatStandard(display.standard);
  const href =
    display.kind === 'token'
      ? getTokenDetailHref(display.id)
      : display.kind === 'identity'
        ? getIdentityItemDetailHref(display.standard, display.id)
        : display.kind === 'object'
          ? display.standard === 'm-nft'
            ? `/objects/mnft/${encodeURIComponent(display.id)}`
            : getObjectDetailHref(display.id)
          : cellHref;
  const tier = display.compositionTier;
  const tierLabel = tier === null ? null : formatCompositionTier(tier);
  const dual = tier === 'btc_ckb';
  const name = display.name ?? display.collectionName;
  return (
    <span className="flex min-w-0 flex-wrap items-baseline gap-x-1.5 gap-y-1 [overflow-wrap:anywhere]">
      <span className="shrink-0">{standard}</span>
      <Link
        href={href}
        className="hover:text-jade min-w-0"
        onClick={(event) => event.stopPropagation()}
      >
        {display.kind === 'token' ? (
          <span className="tabular-nums">
            {display.amount === null
              ? 'Amount unavailable'
              : formatTokenBalanceWithRawMarker(
                  exactUnsignedInteger(display.amount, `${display.id}: token amount`).toString(),
                  display.decimals
                )}{' '}
            {display.symbol ?? display.name ?? (
              <Hash
                hash={display.id}
                startChars={10}
                endChars={6}
                copyable={false}
                className="text-xs"
              />
            )}
          </span>
        ) : (
          <>
            {name && <span>{name} </span>}
            {display.kind !== 'file' || name === null ? (
              <Hash
                hash={display.id}
                startChars={10}
                endChars={6}
                copyable={false}
                className="text-xs"
              />
            ) : null}
          </>
        )}
      </Link>
      {tierLabel && (
        <span
          aria-label={`Media composition: ${tierLabel}`}
          title={
            tier === 'decode_pending'
              ? 'Awaiting DOB execution; media composition has not been classified.'
              : `Media storage composition: ${tierLabel}`
          }
          className={cn(
            'inline-flex shrink-0 items-center gap-1 rounded-full border px-1.5 py-0.5 text-[10px] leading-none',
            !dual &&
              (tier === 'pure_ckb'
                ? 'border-jade/30 text-jade'
                : 'border-base-border text-text-dim')
          )}
          style={
            dual
              ? {
                  color: BITCOIN_COLORS.light,
                  borderColor: 'transparent',
                  background: `linear-gradient(${BITCOIN_COLORS.inner}, ${BITCOIN_COLORS.inner}) padding-box, linear-gradient(135deg, ${BITCOIN_COLORS.light}, ${BITCOIN_COLORS.copper} 35%, #8ff3d6 72%, #1fb88a) border-box`,
                }
              : undefined
          }
        >
          {(dual || tier === 'pure_ckb') && (
            <svg
              aria-hidden="true"
              width="12"
              height="12"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.2"
            >
              <path d="m8 1 6 4v6l-6 4-6-4V5l6-4Zm-6 4 6 4 6-4M8 9v6M8 1v8" />
            </svg>
          )}
          {tierLabel}
        </span>
      )}
    </span>
  );
}
