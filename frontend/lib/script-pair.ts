/**
 * A script by its pair (code hash, hash type): CKB names a script by the pair, so the script
 * lookup is asked, and answers, by the pair (lane rd/api-lock-names). A code hash alone names no
 * script: the same code hash by another hash type is another script.
 */
import type { ScriptPair } from '@/lib/api';

/** The key of a pair in the lookup's answer: `<code hash, lowercase>:<hash type>`. */
export function scriptLookupKey(codeHash: string, hashType: string): string {
  return `${codeHash.toLowerCase()}:${hashType}`;
}

/** The distinct pairs of `scripts` (a cell's absent type script skipped), for one lookup. */
export function distinctScriptPairs(
  scripts: ReadonlyArray<ScriptPair | null | undefined>
): ScriptPair[] {
  const pairs = new Map<string, ScriptPair>();
  for (const script of scripts) {
    if (!script) continue;
    const key = scriptLookupKey(script.codeHash, script.hashType);
    if (!pairs.has(key)) pairs.set(key, { codeHash: script.codeHash, hashType: script.hashType });
  }
  return [...pairs.values()];
}
