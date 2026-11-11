import { formatCount } from '@/lib/format-count';

/** The figures the transaction page's comment is written from: each one the page already shows. */
export interface TxCommentInput {
  /** Cells consumed: the cells section's inputs (none for a cellbase). */
  inputs: number;
  /** Cells created: the cells section's outputs. */
  outputs: number;
  isCellbase: boolean;
  /**
   * The fee as the facts rail prints it (`exactCkbText`, unsigned, no unit: `0.00001647`, `0`);
   * null for a cellbase, which pays none.
   */
  feeCkb: string | null;
  /** Occupied bytes created minus consumed: what the transaction did to common knowledge. */
  knowledgeDeltaBytes: bigint | number;
}

/** `exactCkbText(shannons, false)`: zero, or grouped whole CKB and all eight decimals. */
const EXACT_CKB = /^(?:0|[1-9]\d{0,2}(?:,\d{3})*\.\d{8}|0\.\d{8})$/;

function count(value: number, field: string): number {
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`txComment: ${field} ${value} is not a count of cells`);
  return value;
}

/**
 * The transaction's comment (rubbing × Song print, phase 5, the Transaction board): three lines,
 * lowercase, each a sentence true by construction from the figures the page draws — the cells, the
 * fee, common knowledge. A cellbase says where its cells come from and has no fee line. Anything
 * that would make a line false (a negative count, a cellbase that consumes a cell, a fee that is
 * not the rail's text, a delta that is not a whole number of bytes) throws.
 */
export function txComment({
  inputs,
  outputs,
  isCellbase,
  feeCkb,
  knowledgeDeltaBytes,
}: TxCommentInput): string[] {
  count(inputs, 'inputs');
  count(outputs, 'outputs');
  if (isCellbase && inputs !== 0)
    throw new Error(`txComment: a cellbase consumes no cell, got ${inputs} inputs`);
  if (isCellbase !== (feeCkb === null))
    throw new Error(
      isCellbase
        ? `txComment: a cellbase has no fee line, got fee ${feeCkb}`
        : 'txComment: a transaction that is not a cellbase needs its fee'
    );
  if (feeCkb !== null && !EXACT_CKB.test(feeCkb))
    throw new Error(`txComment: fee "${feeCkb}" is not exact CKB as the facts print it`);
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
  return feeCkb === null ? [cells, knowledge] : [cells, `${feeCkb} ckb to the miner.`, knowledge];
}
