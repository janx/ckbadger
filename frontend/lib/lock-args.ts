import type { ExactJson, LockArgsDecoded } from '@/lib/api';
import { truncateHash } from '@/lib/utils';

// The text of the API's decoded lock args (`lockArgsDecoded`, R2b-a 39 + round 3 R-8): one path
// for every place a lock line shows them. Nothing here decodes bytes; it only names the fields
// the API decoded.

/**
 * Registry field layouts (contracts R-B · R-8): the args are these fields back to back, so they
 * read "args <field> + <field>". `prefix` (the Shadow Lock's first byte, raw) is written as its
 * value alone; the Shadow Lock's 32 bytes are only a `hash` — nothing names what they are.
 */
const FIELD_LAYOUT_LOCKS = new Set(['joyid', 'shadow-lock']);

const FIELD_WORDS: Record<string, string> = {
  subType: 'sub type',
  pubkeyHash: 'pubkey hash',
};

function fieldValue(value: ExactJson): string {
  const text = typeof value === 'string' ? value : JSON.stringify(value);
  return /^0x[0-9a-f]{16,}$/i.test(text) ? truncateHash(text, 8, 4) : text;
}

function fieldWords(key: string): string {
  return FIELD_WORDS[key] ?? key.replace(/([A-Z])/g, ' $1').toLowerCase();
}

/** Whether the args follow a registry field layout (JoyID, Nervape Shadow Lock). */
export function isFieldLayoutLock(decoded: LockArgsDecoded): boolean {
  return typeof decoded.lock === 'string' && FIELD_LAYOUT_LOCKS.has(decoded.lock);
}

/**
 * "args 0x01 + CKBFS type hash 0x31da00...5d74" for a field-layout lock (raw bytes, in layout
 * order, e.g. JoyID's `subType` is shown as given, not interpreted); "pubkey hash 0x…" (fields
 * joined by " · ") for the other decoded families.
 */
/**
 * The one lock line for a lock's args as the API stated them (pass 3, revised R-8): the decoded
 * fields; or, when the args do not fit the registry layout, "args not decoded: <the API's
 * reason>"; or null when the registry declares no layout for the lock (say nothing).
 */
export function lockArgsLine(
  decoded: LockArgsDecoded | null | undefined,
  decodeError: string | null | undefined
): string | null {
  if (decoded && decodeError)
    throw new Error(`lock args both decoded and not decoded (${decodeError})`);
  if (decoded) return decodedLockArgsText(decoded);
  if (decodeError) return `args not decoded: ${decodeError}`;
  return null;
}

export function decodedLockArgsText(decoded: LockArgsDecoded): string {
  const fields = Object.entries(decoded).filter(([key]) => key !== 'lock');
  if (isFieldLayoutLock(decoded))
    return `args ${fields
      .map(([key, value]) =>
        key === 'prefix' ? fieldValue(value) : `${fieldWords(key)} ${fieldValue(value)}`
      )
      .join(' + ')}`;
  return fields.map(([key, value]) => `${fieldWords(key)} ${fieldValue(value)}`).join(' · ');
}
