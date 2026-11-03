import type { ReactNode } from 'react';
import { splitCkbAmount } from '@/lib/ckb-decimal';

// Hierarchy v3 (plan §1b R11, R13; lane C): the pieces of the Scripts pages' heroes, after the DAO
// page's — one figure with its companion on the same baseline row, and one row of quiet metrics.

/**
 * The page's one figure (R4, R11): display from 640 up, index below, bright (scripts have no class
 * colour); the decimals at hero size in ink with the trailing zeros ghosted, the unit at body.
 */
export function CkbFigure({ shannons }: { shannons: bigint }) {
  if (shannons < BigInt(0)) throw new Error(`Script hero figure: negative amount ${shannons}`);
  const parts = splitCkbAmount(shannons);
  return (
    <div data-role="figure-amount" className="text-text-bright text-index sm:text-display">
      <span className="whitespace-nowrap font-mono tabular-nums">
        {parts.integer}
        <span className="text-hero text-text font-normal">
          {parts.fraction}
          <span className="opacity-50">{parts.ghost}</span>
        </span>
      </span>
      {/* The space takes the unit's size, so " CKB" ends inside a 358 px column at 390. */}
      <span className="text-body text-text-dim font-normal"> CKB</span>
    </div>
  );
}

/**
 * The figure block (R13): a meta eyebrow naming the figure, then the figure and its one companion
 * line on the same baseline row (the companion at body, dim).
 */
export function HeroFigure({
  label,
  shannons,
  companion,
}: {
  label: string;
  shannons: bigint;
  companion: ReactNode;
}) {
  return (
    <div data-role="figure" className="mt-9 min-w-0">
      <div className="eyebrow text-text-dim text-meta mb-1.5">{label}</div>
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
        <CkbFigure shannons={shannons} />
        <p className="text-body text-text-dim">{companion}</p>
      </div>
    </div>
  );
}

/** One metric of the hero's quiet row (R13): a meta label over a section value and an inline note. */
export function HeroMetric({
  label,
  children,
  note,
}: {
  label: string;
  children: ReactNode;
  note?: ReactNode;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-meta">{label}</dt>
      <dd className="text-text-bright text-section mt-0.5 [overflow-wrap:anywhere]">
        {children}
        {note && <span className="text-text-dim text-small"> · {note}</span>}
      </dd>
    </div>
  );
}
