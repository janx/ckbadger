import { CkbAmount } from '@/components/ui/ckb-amount';
import { formatAccumulatedRate, type DaoHeader } from '@/lib/dao-header';

/**
 * The four DAO header fields, decoded without economic reinterpretation beyond naming them.
 * S only counts compensation still owed and the burnt share; it holds nothing for anyone. Each line
 * and the note stand on the measure (readability R4, lane B7); the note explains, so it reads in
 * the reading ink at the body step (it was the quiet ink at the raw data's small step, 148
 * characters a line across the header's column).
 */
export function DaoHeaderFields({ dao }: { dao: DaoHeader }) {
  return (
    <div className="mt-2 space-y-2" aria-label="Decoded DAO header">
      <p className="max-w-measure">
        <span className="text-text-dim">C · total issued </span>
        <CkbAmount form="column" shannons={dao.C} signed={false} tone="plain" />
      </p>
      <p className="max-w-measure">
        <span className="text-text-dim">AR · accumulated rate </span>
        {formatAccumulatedRate(dao.AR)}
      </p>
      <p className="max-w-measure">
        <span className="text-text-dim">S · compensation still owed + the burnt share </span>
        <CkbAmount form="column" shannons={dao.S} signed={false} tone="plain" />
      </p>
      <p className="max-w-measure">
        <span className="text-text-dim">U · protocol occupied </span>
        <CkbAmount form="column" shannons={dao.U} signed={false} tone="plain" />
      </p>
      <p className="text-text text-body max-w-measure">
        S counts the burnt share alongside compensation still owed to live deposits; it holds
        nothing for anyone. U includes genesis virtual occupied capacity. These are the four
        little-endian u64 header fields.
      </p>
    </div>
  );
}
