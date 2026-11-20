/**
 * What the route states of every party of an activity (lane feed, ruling 14 of gate P; docs/API.md,
 * "The parties a row draws"): a row sends in full only the parties it draws (every party of a row
 * of at most six; beyond, the five largest CKB changes, the fee payer and every party whose items
 * moved), so what a page says of all of them it reads here, computed by the route once from all of
 * them, never from `participants`: how many there are, each by its short id (the fold counts
 * distinct parties from them), and the party whose only movement is the fee. Each reader refuses a
 * response without its field, as an API older than this page (`sentField`).
 */
import type { PartyId } from '@/lib/api';
import { sentField } from '@/lib/api-fields';

/** A row as the route sends it: its transaction, named where a field is refused. */
interface StatedRow {
  txHash: string;
  participantCount?: number;
  partyIds?: readonly PartyId[];
  feePayer?: string | null;
}

const where = (row: StatedRow) => `transaction ${row.txHash}`;

/** How many parties the transaction has, exactly (the route's `participantCount`). */
export function participantCount(row: StatedRow): number {
  const count = sentField(row.participantCount, where(row), 'participantCount');
  if (!Number.isSafeInteger(count) || count < 0)
    throw new Error(`${where(row)}: a participant count of ${count}`);
  return count;
}

/**
 * Every party of the transaction by its short id and tags, in its order (the route's `partyIds`):
 * as many as it has parties, each once.
 */
export function partyIds(row: StatedRow): readonly PartyId[] {
  const ids = sentField(row.partyIds, where(row), 'partyIds');
  const count = participantCount(row);
  if (ids.length !== count)
    throw new Error(`${where(row)}: it names ${ids.length} of its ${count} parties`);
  return ids;
}

/** The lock hash of the party whose only movement is the fee, or null (the route's `feePayer`). */
export function feePayer(row: StatedRow): string | null {
  return sentField(row.feePayer, where(row), 'feePayer');
}

/**
 * Whether the party the route names by `id` holds the lock `lockHash`: its short id is the first 20
 * bytes of its lock hash, so the lock hash begins with it.
 */
export function namesLock(id: string, lockHash: string): boolean {
  return lockHash.startsWith(id);
}
