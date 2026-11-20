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
 * What an identity is and whether it is live, by its standard and its state (readability R8): the
 * one sentence of "in words" that says so, as the page's lede, in place of a sentence of where the
 * standard keeps a name, which contradicted a name no cell holds ("The name and everything about it
 * live in one CKB cell." beside "0 CKB · no live cell"). It states no figure: the dates and the CKB
 * are the metrics row's (the lede rule, C7). A state the standard does not have has no sentence.
 */
const STATE_SENTENCES: Record<IdentityStandard, Partial<Record<IdentityState, string>>> = {
  dotbit: {
    live: 'A live .bit name: it and everything about it live in one CKB cell.',
    expired: 'A .bit name whose term has ended; its CKB cell still holds it.',
    recycled: 'A .bit name that is no longer a live account: no CKB cell holds it now.',
  },
  bit_cell: {
    live: 'A live .bit Cell: a .bit name made into a cell of its own, held in an ordinary wallet.',
    expired: 'A .bit Cell whose name’s term has ended; the cell is still live.',
    inactive: 'A .bit Cell that is no longer live: no cell holds the name now.',
  },
  did_ckb: {
    live: 'A live Web5 identity: its DID document lives in this CKB cell, with no expiry.',
    inactive: 'A Web5 identity that is no longer live: no CKB cell holds its DID document now.',
  },
  dotcell: {
    active: 'A live .cell name: it lives in a CKB cell, its records in the witness beside it.',
    grace: 'A .cell name whose term has ended, in its grace period; its cell is still live.',
    free: 'A .cell name past its term and its grace period, free to register; its cell is still live.',
    recycled: 'A .cell name that is no longer live: no CKB cell holds it now.',
  },
};
/** A .bit Cell past its term whose .bit account is gone too (the review's plainest sentence). */
const BIT_CELL_ACCOUNT_GONE =
  'A .bit Cell whose name’s term has ended; the .bit account is no longer live, but this cell remains.';

/** Where each standard keeps its names, said of a whole standard on its collection page (ID-COL-5). */
const COLLECTION_POSITIONING: Record<IdentityStandard, string> = {
  dotbit: 'Each .bit name and everything about it live in one CKB cell.',
  bit_cell: 'Each is a .bit name made into a cell of its own, held in an ordinary wallet.',
  did_ckb: 'Each did:ckb identity keeps its DID document in a CKB cell.',
  dotcell: 'Each .cell name lives in a CKB cell; its records ride in the witness beside it.',
};

/**
 * The lede of an identity page or of an identity collection. Every one of the four standards lives
 * in CKB cells (`.cell` records in the witness beside its cell), so all carry the Pure CKB badge
 * (identity boards). On an identity's page the sentence is its state (R8), read before the badge,
 * the badge the small one after it; on a collection's, where the standard keeps its names, the
 * badge first.
 */
export function IdentityPositioning(
  props:
    | {
        standard: IdentityStandard;
        scope: 'collection';
      }
    | {
        standard: IdentityStandard;
        scope?: 'item';
        state: IdentityState;
        /** A .bit Cell whose .bit account is no longer live. */
        accountGone?: boolean;
      }
) {
  const { standard } = props;
  if (props.scope === 'collection') {
    const text = COLLECTION_POSITIONING[standard];
    if (!text) throw new Error(`No identity standard ${JSON.stringify(standard)}`);
    return (
      <div className="flex flex-wrap items-center gap-3">
        <CompositionBadge tier="pure_ckb" />
        <span className="text-text text-body max-w-measure">{text}</span>
      </div>
    );
  }
  const { state, accountGone = false } = props;
  if (accountGone && !(standard === 'bit_cell' && state === 'expired'))
    throw new Error(
      `An identity account gone is a .bit Cell past its term, not ${standard} ${state}`
    );
  const text = accountGone ? BIT_CELL_ACCOUNT_GONE : STATE_SENTENCES[standard]?.[state];
  if (!text)
    throw new Error(`No state sentence for a ${standard} identity ${JSON.stringify(state)}`);
  return (
    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1.5">
      <p data-testid="identity-state-sentence" className="text-text text-body max-w-measure">
        {text}
      </p>
      <CompositionBadge tier="pure_ckb" size="sm" className="self-center" />
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
  // R8: an ended state reads in the reading ink with a border that shows (it was the quiet ink on
  // a 1.15 : 1 border, the head's weakest mark, under the Pure CKB badge every identity carries)
  free: { label: 'Free to register', className: 'text-text border-text-dim border-dashed' },
  recycled: { label: 'Recycled', className: 'text-text border-text-dim' },
  inactive: { label: 'Inactive', className: 'text-text border-text-dim' },
};

export function IdentityStatusBadge({ state }: { state: IdentityState }) {
  const s = STATE[state];
  if (!s) throw new Error(`Unknown identity state ${JSON.stringify(state)}`);
  return (
    <span data-identity-state={state} className={cn('text-meta border px-1.5 py-px', s.className)}>
      {s.label}
    </span>
  );
}
