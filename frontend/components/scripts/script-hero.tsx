import type { ReactNode } from 'react';
import { splitCkbAmount } from '@/lib/ckb-decimal';
import { Dotted } from '@/components/ui/phone-row';
import { unitNode } from '@/components/ui/units';

// Hierarchy v3 (plan §1b R11, R13; lane C): the pieces of the Scripts pages' heroes, after the DAO
// page's — one figure with its companion on the same baseline row, and one row of quiet metrics.

/**
 * The page's one figure (R4, R11): display from 640 up, index below, bright (scripts have no class
 * colour); the decimals in ink with the trailing zeros ghosted, the unit at body. The figure and
 * its unit are one unbreakable unit (RESPONSIVE_LAYOUT, a number with its unit; PA-19). Below 640
 * the decimals are body, not hero: the integer of 49,890,081,314 CKB (14 characters at 26 px,
 * 218.4 px), its 9 decimals at 22 px (118.8) and " CKB" (31.2) took 368.4 px, and at 17 px still
 * 341.4, against the 326 px line at 360; at 13 px (70.2) they take 319.8.
 */
export function CkbFigure({ shannons }: { shannons: bigint }) {
  if (shannons < BigInt(0)) throw new Error(`Script hero figure: negative amount ${shannons}`);
  const parts = splitCkbAmount(shannons);
  return (
    <div data-role="figure-amount" className="text-text-bright text-index sm:text-display">
      <span className="whitespace-nowrap font-mono tabular-nums">
        {parts.integer}
        <span className="text-body sm:text-hero text-text font-normal">
          {parts.fraction}
          <span className="text-text-ghost">{parts.ghost}</span>
        </span>
        {/* The space takes the unit's size. */}
        <span className="text-body text-text-dim font-normal"> CKB</span>
      </span>
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
  /** The companion line's parts, one separated list (PA-25). */
  companion: readonly ReactNode[];
}) {
  return (
    <div data-role="figure" className="mt-9 min-w-0">
      <div className="eyebrow text-text-dim text-meta mb-1.5">{label}</div>
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
        <CkbFigure shannons={shannons} />
        <p className="text-body text-text-dim">
          <Dotted parts={companion} />
        </p>
      </div>
    </div>
  );
}

/**
 * One metric of the hero's quiet row (R13): a meta label over a section value and an inline note.
 * The value (or its `parts`) and the note are one separated list (PA-25): a note's dot goes with
 * the note to a new line, never left at a line's end. The list speaks in the note's size, so its
 * dots and their spacing are one size; each value part is set at section size inside its item.
 */
export function HeroMetric({
  label,
  children,
  parts,
  note,
}: {
  label: string;
  children?: ReactNode;
  /** A value said in parts ("Upgradable · 5 versions"), in place of `children`. */
  parts?: readonly ReactNode[];
  note?: ReactNode;
}) {
  const value = parts ?? [children];
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-meta">{label}</dt>
      <dd className="mt-0.5 [overflow-wrap:anywhere]">
        <Dotted
          className="text-small"
          parts={[
            ...value.map((part, i) => (
              <span key={i} className="text-text-bright text-section">
                {unitNode(part)}
              </span>
            )),
            note && <span className="text-text-dim">{unitNode(note)}</span>,
          ]}
        />
      </dd>
    </div>
  );
}
