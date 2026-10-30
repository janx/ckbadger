/**
 * Exact parts of an exact total laid side by side over `width` units from `x0`. The parts must
 * partition the total exactly (no remainder, no negatives); geometry alone uses floating point.
 */
export function partitionSpans(
  parts: readonly bigint[],
  total: bigint,
  x0: number,
  width: number,
  context: string
): { x: number; width: number }[] {
  if (total <= BigInt(0)) throw new Error(`${context}: total ${total} is not positive`);
  for (const part of parts)
    if (part < BigInt(0)) throw new Error(`${context}: negative part ${part}`);
  const sum = parts.reduce((a, b) => a + b, BigInt(0));
  if (sum !== total) throw new Error(`${context}: parts sum to ${sum}, not the total ${total}`);
  // Scale by a fixed integer resolution so the float conversion happens once per edge.
  const RES = BigInt(1_000_000_000);
  const edge = (upTo: bigint) => x0 + (Number((upTo * RES) / total) / 1e9) * width;
  let before = BigInt(0);
  return parts.map((part) => {
    const x = edge(before);
    before += part;
    const end = edge(before);
    return { x: round(x), width: round(end - x) };
  });
}

const round = (n: number) => Math.round(n * 100) / 100;
