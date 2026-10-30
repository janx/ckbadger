/** `1 block`, `2 blocks`, `1,234 confirmations`: the noun agrees with the count (FF-D6, TX-34). */
export function formatCount(
  count: number | bigint,
  singular: string,
  plural = `${singular}s`
): string {
  const valid =
    typeof count === 'bigint' ? count >= BigInt(0) : Number.isSafeInteger(count) && count >= 0;
  if (!valid) throw new Error(`Invalid count ${count} for ${singular}`);
  const one = typeof count === 'bigint' ? count === BigInt(1) : count === 1;
  return `${count.toLocaleString('en-US')} ${one ? singular : plural}`;
}
