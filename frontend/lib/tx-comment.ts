import { formatCount } from '@/lib/format-count';

/** The figures the transaction page's comment is written from: each one the page already shows. */
export interface TxCommentInput {
  /** Cells consumed: the cells section's inputs (none for a cellbase). */
  inputs: number;
  /** Cells created: the cells section's outputs. */
  outputs: number;
  isCellbase: boolean;
  /** Occupied bytes created minus consumed: what the transaction did to common knowledge. */
  knowledgeDeltaBytes: bigint | number;
}

function count(value: number, field: string): number {
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`txComment: ${field} ${value} is not a count of cells`);
  return value;
}

/**
 * The transaction's comment (rubbing × Song print, phase 5, the Transaction board): two lines,
 * lowercase, each a sentence true by construction from the figures the page draws — the cells and
 * common knowledge. A cellbase says where its cells come from. The fee is not a line of it: a fact
 * is said once, and the fee stands in the status strip under the id line and in the created sheet's
 * balance row (lane D2, the board passed at gate D0′; the comment said "0.00064119 ckb to the
 * miner." between the two). Anything that would make a line false (a negative count, a cellbase
 * that consumes a cell, a delta that is not a whole number of bytes) throws.
 */
export function txComment({
  inputs,
  outputs,
  isCellbase,
  knowledgeDeltaBytes,
}: TxCommentInput): string[] {
  count(inputs, 'inputs');
  count(outputs, 'outputs');
  if (isCellbase && inputs !== 0)
    throw new Error(`txComment: a cellbase consumes no cell, got ${inputs} inputs`);
  if (typeof knowledgeDeltaBytes === 'number' && !Number.isSafeInteger(knowledgeDeltaBytes))
    throw new Error(`txComment: common knowledge delta ${knowledgeDeltaBytes} is not whole bytes`);

  const cells = isCellbase
    ? `${formatCount(outputs, 'cell')} from the block reward; none consumed.`
    : `${formatCount(inputs, 'cell')} became ${outputs.toLocaleString('en-US')}.`;
  const delta = BigInt(knowledgeDeltaBytes);
  const knowledge =
    delta > BigInt(0)
      ? `common knowledge grew by ${formatCount(delta, 'byte')}.`
      : delta < BigInt(0)
        ? `common knowledge shrank by ${formatCount(-delta, 'byte')}.`
        : 'common knowledge unchanged.';
  return [cells, knowledge];
}
