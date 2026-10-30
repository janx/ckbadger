/**
 * The one way the site writes a block height: `#20,617,704` (DAO-9, SC-C10, FF-L3, SC-S8).
 * A value that is not a non-negative integer height is a contract violation and throws.
 */
export function formatBlockNumber(value: number | bigint | string): string {
  let height: bigint;
  if (typeof value === 'bigint') height = value;
  else if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new Error(`Invalid block number ${value}`);
    height = BigInt(value);
  } else {
    if (!/^(0|[1-9]\d*)$/.test(value))
      throw new Error(`Invalid block number ${JSON.stringify(value)}`);
    height = BigInt(value);
  }
  if (height < BigInt(0)) throw new Error(`Invalid block number ${value}`);
  return `#${height.toLocaleString('en-US')}`;
}
