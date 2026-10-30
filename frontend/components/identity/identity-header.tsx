import { CompositionBadge } from '@/components/ui/composition-mark';
import { cn } from '@/lib/utils';

export type IdentityStandard = 'dotbit' | 'bit_cell' | 'did_ckb' | 'dotcell';

/** A name split at its standard's suffix (`valar` + `.bit`); a name without it keeps no suffix. */
export function identityNameParts(full: string, suffix: string): { name: string; suffix: string } {
  return full.length > suffix.length && full.endsWith(suffix)
    ? { name: full.slice(0, -suffix.length), suffix }
    : { name: full, suffix: '' };
}

/** The title as the identity boards set it: the name bright, the standard's suffix in rose. */
export function IdentityName({ name, suffix }: { name: string; suffix: string }) {
  return (
    <>
      {name}
      {suffix && <span className="text-identity">{suffix}</span>}
    </>
  );
}

/**
 * Where each standard keeps its identity. Every one of the four lives in CKB cells (`.cell`
 * records in the witness beside its cell), so all carry the Pure CKB badge (identity boards).
 */
const POSITIONING: Record<IdentityStandard, string> = {
  dotbit: 'The name and everything about it live in one CKB cell.',
  bit_cell: 'A .bit name made into a cell of its own, held in an ordinary wallet.',
  did_ckb: 'Its DID document lives in this CKB cell.',
  dotcell: 'The name lives in a CKB cell; its records ride in the witness beside it.',
};

export function IdentityPositioning({ standard }: { standard: IdentityStandard }) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <CompositionBadge tier="pure_ckb" />
      <span className="text-text text-[12.5px]">{POSITIONING[standard]}</span>
    </div>
  );
}

export type IdentityState =
  | 'live'
  | 'active'
  | 'grace'
  | 'expired'
  | 'free'
  | 'recycled'
  | 'inactive';

const STATE: Record<IdentityState, { label: string; className: string }> = {
  live: { label: 'Live', className: 'text-jade border-jade/40' },
  active: { label: 'Active', className: 'text-jade border-jade/40' },
  // Ended or ending terms are neutral grey and dashed: gold is DAO-only (FINAL_REVIEW).
  grace: { label: 'In grace period', className: 'text-text border-text-dim border-dashed' },
  expired: { label: 'Expired', className: 'text-text border-text-dim border-dashed' },
  free: { label: 'Free to register', className: 'text-text-dim border-text-dim border-dashed' },
  recycled: { label: 'Recycled', className: 'text-text-dim border-base-border' },
  inactive: { label: 'Inactive', className: 'text-text-dim border-base-border' },
};

export function IdentityStatusBadge({ state }: { state: IdentityState }) {
  const s = STATE[state];
  if (!s) throw new Error(`Unknown identity state ${JSON.stringify(state)}`);
  return (
    <span
      data-identity-state={state}
      className={cn('rounded border px-1.5 py-px text-[11px]', s.className)}
    >
      {s.label}
    </span>
  );
}
