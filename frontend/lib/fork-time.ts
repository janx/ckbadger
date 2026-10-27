/** This node's detection clock, aligned with the overview's fixed UTC+8 bins. */
export function formatObserverTimeUtc8(timestamp: string): string {
  const millis = Date.parse(timestamp);
  if (!Number.isFinite(millis)) throw new Error(`invalid fork detection time: ${timestamp}`);
  const shifted = new Date(millis + 8 * 60 * 60 * 1000);
  if (!Number.isFinite(shifted.getTime()))
    throw new Error(`fork detection time exceeds display range: ${timestamp}`);
  return `${shifted.toISOString().slice(0, 19).replace('T', ' ')} UTC+8`;
}
