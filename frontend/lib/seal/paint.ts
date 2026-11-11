/**
 * The paints a seal is drawn in. A seal's markup is set as inner HTML, so the only colours that
 * may enter it are a hex colour or a palette reference (`var(--c-…)`, `lib/palette.ts`); anything
 * else is refused with the offending value named.
 */

const PAINT = /^(#[0-9a-f]{6}|var\(--c-[a-z0-9-]+\))$/i;

export function assertPaint(name: string, value: string): void {
  if (typeof value !== 'string' || !PAINT.test(value))
    throw new Error(
      `hash seal: ${name} must be a hex colour or a var(--c-…) palette reference, got ${JSON.stringify(value)}`
    );
}
