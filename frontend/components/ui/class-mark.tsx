import type { ReactNode } from 'react';
import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';

/** The mark sizes of the AssetClasses spec board: 12 (inline item), 16 (rows, chips), 24 (titles). */
export type MarkSize = 12 | 16 | 24;

/** The approved class marks (mockup/marks/*.svg): shape carries the class, colour helps. */
const SHAPES: Record<AssetClass, (color: string) => ReactNode> = {
  dao: (c) => (
    <>
      <circle cx="8" cy="8" r="6" fill="none" stroke={c} strokeWidth="1.6" />
      <circle cx="8" cy="8" r="2.4" fill={c} />
    </>
  ),
  tokens: (c) => (
    <>
      <ellipse cx="8" cy="5" rx="5.5" ry="2.3" fill={c} />
      <path
        d="M2.5 5v6c0 1.3 2.5 2.3 5.5 2.3s5.5-1 5.5-2.3V5"
        fill="none"
        stroke={c}
        strokeWidth="1.5"
      />
      <path
        d="M2.5 8.2c0 1.3 2.5 2.3 5.5 2.3s5.5-1 5.5-2.3"
        fill="none"
        stroke={c}
        strokeWidth="1.2"
        strokeOpacity="0.7"
      />
    </>
  ),
  objects: (c) => (
    <>
      <path
        d="M8 1.8 13.6 5v6L8 14.2 2.4 11V5z"
        fill={c}
        fillOpacity="0.2"
        stroke={c}
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <path d="M2.4 5 8 8.2 13.6 5M8 8.2v6" fill="none" stroke={c} strokeWidth="1.2" />
    </>
  ),
  identities: (c) => (
    <>
      <circle cx="8" cy="5.6" r="2.8" fill={c} />
      <path
        d="M2.8 14c.6-2.9 2.7-4.4 5.2-4.4s4.6 1.5 5.2 4.4"
        fill="none"
        stroke={c}
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </>
  ),
  // Neutrals: plain CKB and other scripts have no class mark of their own on the board.
  ckb: (c) => (
    <rect x="3" y="3" width="10" height="10" rx="1" fill="none" stroke={c} strokeWidth="1.2" />
  ),
  other: (c) => <path d="m5 3-4 5 4 5m6-10 4 5-4 5" fill="none" stroke={c} strokeWidth="1.2" />,
};

export function ClassMark({
  kind,
  size = 16,
  className,
}: {
  kind: AssetClass;
  size?: MarkSize;
  className?: string;
}) {
  return (
    <svg
      role="img"
      aria-label={`${kind} class`}
      data-class={kind}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      className={cn('inline-block shrink-0 align-middle', className)}
    >
      {SHAPES[kind](CLASS_COLORS[kind])}
    </svg>
  );
}
