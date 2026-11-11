/**
 * The epoch a block header states, decoded from its packed `epoch` field: the epoch's `number`,
 * the block's `index` in it and the epoch's `length`. The API writes it `number(index/length)`,
 * e.g. `15168(451/1590)`.
 */
export interface EpochInfo {
  number: number;
  index: number;
  length: number;
}

const EPOCH_RE = /^(\d+)\((\d+)\/(\d+)\)$/;

/**
 * CKB's genesis epoch, `EpochNumberWithFraction::is_genesis`: number, index and length all 0. CKB
 * builds the genesis header with exactly this value (ckb-chain-spec `build_genesis`:
 * `EpochNumberWithFraction::new_unchecked(0, 0, 0)`), so its packed field is `0x0`.
 */
export function isGenesisEpoch({ number, index, length }: EpochInfo): boolean {
  return number === 0 && index === 0 && length === 0;
}

/**
 * The one rule for the epoch block `block`'s header states, returning it. Every header after
 * genesis passes CKB's header verifier, whose `is_well_formed` asks index < length; the genesis
 * header, which that verifier never checks, carries the zero epoch, the one header on a chain with
 * length 0. So the zero epoch is accepted as block 0's and refused on any other block (there it is
 * a decode or write bug, not the chain), and every other index at or past its length is refused.
 * Whether block 0's header is the chain spec's genesis is the node's check, by its hash, not this
 * rule's. Every caller knows the block whose header it reads, so the block is required.
 */
export function checkHeaderEpoch(epoch: EpochInfo, block: number): EpochInfo {
  if (!Number.isSafeInteger(block) || block < 0)
    throw new Error(`block ${block} is not a non-negative integer`);
  const { number, index, length } = epoch;
  for (const [name, value] of [
    ['number', number],
    ['index', index],
    ['length', length],
  ] as const)
    if (!Number.isSafeInteger(value) || value < 0)
      throw new Error(`block ${block}: epoch ${name} ${value} is not a non-negative integer`);
  const written = `${number}(${index}/${length})`;
  if (isGenesisEpoch(epoch)) {
    if (block !== 0)
      throw new Error(
        `block ${block}: epoch ${written} is the genesis header’s alone; every later header states its epoch’s length`
      );
    return epoch;
  }
  if (length === 0) throw new Error(`block ${block}: epoch ${written} has length 0`);
  if (index >= length)
    throw new Error(`block ${block}: epoch index ${index} is outside its length ${length}`);
  return epoch;
}

/**
 * Parse `/statistics/network`'s `epoch` field, the epoch of the tip header, block `block` (the
 * response's `latestBlock`). A string that is not `number(index/length)`, or an epoch the header
 * rule refuses, is an API contract violation: it throws with the raw value rather than inventing an
 * epoch.
 */
export function parseEpoch(raw: string, block: number): EpochInfo {
  const match = EPOCH_RE.exec(raw);
  if (!match) {
    throw new Error(`epoch "${raw}" is not number(index/length)`);
  }
  return checkHeaderEpoch(
    { number: Number(match[1]), index: Number(match[2]), length: Number(match[3]) },
    block
  );
}

/**
 * One written form of the epoch position block `block`'s header states: `15,173 · 1,001 / 1,472`
 * (BA-41). Genesis's is what its header states, `0 · 0 / 0`; the page says why beside it.
 */
export function formatEpochPosition(epoch: EpochInfo, block: number): string {
  const { number, index, length } = checkHeaderEpoch(epoch, block);
  const n = (v: number) => v.toLocaleString('en-US');
  return `${n(number)} · ${n(index)} / ${n(length)}`;
}
