import { ExactCkb } from '@/components/activity-event-row';
import { formatAccumulatedRate, type DaoHeader } from '@/lib/dao-header';

/**
 * The four DAO header fields, decoded without economic reinterpretation beyond naming them.
 * S only counts compensation still owed and the burnt share; it holds nothing for anyone.
 */
export function DaoHeaderFields({ dao }: { dao: DaoHeader }) {
  return (
    <div className="mt-2 space-y-2" aria-label="Decoded DAO header">
      <p>
        <span className="text-text-dim">C · total issued </span>
        <ExactCkb shannons={dao.C} signed={false} tone="plain" />
      </p>
      <p>
        <span className="text-text-dim">AR · accumulated rate </span>
        {formatAccumulatedRate(dao.AR)}
      </p>
      <p>
        <span className="text-text-dim">S · compensation still owed + the burnt share </span>
        <ExactCkb shannons={dao.S} signed={false} tone="plain" />
      </p>
      <p>
        <span className="text-text-dim">U · protocol occupied </span>
        <ExactCkb shannons={dao.U} signed={false} tone="plain" />
      </p>
      <p className="text-text-dim">
        S counts the burnt share alongside compensation still owed to live deposits; it holds
        nothing for anyone. U includes genesis virtual occupied capacity. These are the four
        little-endian u64 header fields.
      </p>
    </div>
  );
}
