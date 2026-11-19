/**
 * The link to a transaction at one of its cells (review of phases C and D, SHOULD 6; lane P1): the
 * transaction page folds each sheet after six cells and draws, wherever it stands, the cell its
 * URL's anchor names by its out point (`#<tx hash>-<index>`, lane D3). So a link into a
 * transaction from the cell it is about carries that anchor, or the fold can hide the very cell the
 * reader came from. Every such link is built here, one form for the anchor the page reads.
 */

const HASH = /^0x[0-9a-fA-F]{64}$/;

function checkedHash(hash: string, what: string): string {
  if (!HASH.test(hash)) throw new Error(`${what} ${JSON.stringify(hash)} is not a 32-byte hash`);
  return hash.toLowerCase();
}

/** A cell's out point as the transaction page's anchor writes it: `0x…-57`, the hash lowercase. */
function cellAnchor(cell: { txHash: string; index: number }): string {
  if (!Number.isSafeInteger(cell.index) || cell.index < 0)
    throw new Error(`cell index ${cell.index} of ${cell.txHash} is not an output index`);
  return `${checkedHash(cell.txHash, 'cell transaction hash')}-${cell.index}`;
}

/**
 * `/tx/<txHash>#<cell's out point>`: the transaction `txHash` (the one that created the cell, or
 * the one that spent it) opened at `cell`, which its page then never folds away. A hash that is
 * not one, or an index that is not an output's, is refused, never written into a link.
 */
export function transactionAtCellHref(
  txHash: string,
  cell: { txHash: string; index: number }
): string {
  return `/tx/${checkedHash(txHash, 'transaction hash')}#${cellAnchor(cell)}`;
}
