import { useId, type ReactNode } from 'react';
import { COMPOSITION_BADGES, COMPOSITION_COLORS } from '@/lib/design-tokens';
import { formatCompositionTier, type CompositionTier } from '@/lib/asset-utils';
import type { MarkSize } from '@/components/ui/class-mark';
import { cn } from '@/lib/utils';

const { crystal: C, copperMount: M, offchain: GREY, offchainFill: GROUND } = COMPOSITION_COLORS;

function Crystal({ mounted }: { mounted: boolean }) {
  // Unmounted, the crystal fills the square; in the copper mount it is shorter and sits above it.
  const d = mounted
    ? [
        'M8 0.8 5.2 3.9 8 5.3z',
        'M8 0.8 10.8 3.9 8 5.3z',
        'M5.2 3.9 8 5.3V12.2L5.2 11z',
        'M10.8 3.9 8 5.3V12.2L10.8 11z',
      ]
    : [
        'M8 0.8 4.4 4.6 8 6.4z',
        'M8 0.8 11.6 4.6 8 6.4z',
        'M4.4 4.6 8 6.4V15.2L4.4 12.6z',
        'M11.6 4.6 8 6.4V15.2L11.6 12.6z',
      ];
  return (
    <g stroke={C.edge} strokeWidth="0.35" strokeLinejoin="round">
      <path d={d[0]} fill={C.top} />
      <path d={d[1]} fill={C.topLight} />
      <path d={d[2]} fill={C.left} />
      <path d={d[3]} fill={C.right} />
    </g>
  );
}
const GLINT = 'M13.4 1.1l.4.95.95.4-.95.4-.4.95-.4-.95-.95-.4.95-.4z';

function CopperMount() {
  const id = useId().replace(/:/g, '');
  const top = `copper-top-${id}`,
    front = `copper-front-${id}`;
  return (
    <>
      <defs>
        <linearGradient id={top} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor={M.topLight} />
          <stop offset="0.55" stopColor={M.topMid} />
          <stop offset="1" stopColor={M.topDark} />
        </linearGradient>
        <linearGradient id={front} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={M.frontLight} />
          <stop offset="0.5" stopColor={M.frontMid} />
          <stop offset="1" stopColor={M.frontDark} />
        </linearGradient>
      </defs>
      <Crystal mounted />
      <path
        d="M5.2 10.3 4.4 7.6 6 10.3zM10.8 10.3 11.6 7.6 10 10.3z"
        fill={`url(#${top})`}
        stroke={M.edge}
        strokeWidth="0.3"
        strokeLinejoin="round"
      />
      <path
        d="M3.4 10.2h9.2l-1.1 4.8H4.5z"
        fill={`url(#${front})`}
        stroke={M.edge}
        strokeWidth="0.35"
        strokeLinejoin="round"
      />
      <path d="M3.4 10.2h9.2l-.35 1.1H3.75z" fill={`url(#${top})`} />
      <path d={GLINT} fill={C.glint} />
    </>
  );
}

const unknownGlyph = (
  <>
    <circle
      cx="8"
      cy="8"
      r="6"
      fill="none"
      stroke={GREY}
      strokeWidth="1.1"
      strokeDasharray="1.8 1.4"
    />
    <path
      d="M6.4 6.3a1.7 1.7 0 1 1 2.3 1.6c-.5.2-.7.5-.7 1v.3M8 11v.1"
      fill="none"
      stroke={GREY}
      strokeWidth="1.1"
      strokeLinecap="round"
    />
  </>
);

const GLYPHS: Record<CompositionTier, () => ReactNode> = {
  pure_ckb: () => (
    <>
      <Crystal mounted={false} />
      <path
        d="M9.3 7.6v5.2"
        stroke={C.glint}
        strokeOpacity="0.6"
        strokeWidth="0.6"
        strokeLinecap="round"
      />
      <path d={GLINT} fill={C.glint} />
    </>
  ),
  btc_ckb: () => <CopperMount />,
  decentralized_mixture: () => (
    <g fill="none" stroke={GREY} strokeWidth="1.2">
      <path d="M4 11.5 8 4.5 12 11.5z" />
      <circle cx="8" cy="4.5" r="1.8" fill={GROUND} />
      <circle cx="4" cy="11.5" r="1.8" fill={GROUND} />
      <circle cx="12" cy="11.5" r="1.8" fill={GROUND} />
    </g>
  ),
  centralized_mixture: () => (
    <>
      <g fill="none" stroke={GREY} strokeWidth="1.2">
        <rect x="3" y="2.5" width="10" height="4.6" rx="1" />
        <rect x="3" y="8.9" width="10" height="4.6" rx="1" />
      </g>
      <circle cx="5.4" cy="4.8" r="0.8" fill={GREY} />
      <circle cx="5.4" cy="11.2" r="0.8" fill={GREY} />
    </>
  ),
  unknown: () => unknownGlyph,
  // Not classified yet: the same "not determined" outline, labelled as pending beside it.
  decode_pending: () => unknownGlyph,
};

const MARK_LABELS: Record<CompositionTier, string> = {
  pure_ckb: 'Pure CKB',
  btc_ckb: 'BTC+CKB',
  decentralized_mixture: 'Decentralized mix',
  centralized_mixture: 'Centralized mix',
  unknown: 'Unknown composition',
  decode_pending: 'Decode pending',
};

/** The composition mark of an object (16 px by default), drawn from mockup/marks. */
export function CompositionMark({
  tier,
  size = 16,
  className,
}: {
  tier: CompositionTier;
  size?: MarkSize;
  className?: string;
}) {
  const glyph = GLYPHS[tier];
  if (!glyph) throw new Error(`Unknown object composition tier ${JSON.stringify(tier)}`);
  return (
    <svg
      role="img"
      aria-label={MARK_LABELS[tier]}
      data-composition-mark={tier}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      className={cn('inline-block shrink-0 align-middle', className)}
    >
      {glyph()}
    </svg>
  );
}

/**
 * Mark + short label, one drawing site-wide (IV-24): the on-chain tiers in their gradient frame
 * with gradient ink, the off-chain tiers deliberately plain.
 */
export function CompositionBadge({
  tier,
  size = 'md',
  className,
}: {
  tier: CompositionTier;
  /** `sm` for tx tiles and dense rows, `md` elsewhere. */
  size?: 'sm' | 'md';
  className?: string;
}) {
  const label = formatCompositionTier(tier);
  const markSize: MarkSize = size === 'sm' ? 12 : 16;
  if (tier === 'pure_ckb' || tier === 'btc_ckb') {
    const style = COMPOSITION_BADGES[tier];
    return (
      <span
        data-composition={tier}
        className={cn(
          'inline-flex shrink-0 items-center gap-[5px] whitespace-nowrap rounded-full border border-transparent font-medium',
          size === 'sm'
            ? 'h-[18px] pl-1 pr-1.5 text-[10px]'
            : 'h-[22px] pl-[5px] pr-[9px] text-[11px]',
          className
        )}
        style={{
          backgroundImage: style.frame,
          backgroundClip: 'padding-box, border-box',
          backgroundOrigin: 'padding-box, border-box',
        }}
      >
        <CompositionMark tier={tier} size={markSize} />
        <span
          className="bg-clip-text text-transparent"
          style={{ backgroundImage: style.ink, WebkitBackgroundClip: 'text' }}
        >
          {label}
        </span>
      </span>
    );
  }
  return (
    <span
      data-composition={tier}
      className={cn(
        'text-text-dim inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap',
        size === 'sm' ? 'text-[10px]' : 'text-[12px]',
        className
      )}
      title={
        tier === 'decode_pending'
          ? 'Awaiting DOB execution; composition has not been classified.'
          : undefined
      }
    >
      <CompositionMark tier={tier} size={markSize} />
      {label}
    </span>
  );
}
