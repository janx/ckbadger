'use client';

import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useRouter, useSearchParams } from '@/src/navigation';
import { api, type ScriptLookupInfo } from '@/lib/api';
import { scriptLookupKey } from '@/lib/script-pair';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { NotFoundPage } from '@/components/not-found-page';
import Link from '@/components/ui/link';
import { getScriptDetailHref } from '@/lib/detail-routes';

const UNKNOWN_SCRIPT_NAME = 'unknown';
/** What `/script/` takes besides a script name, as the not-an-id page says it. */
const CODE_HASH_RULE = 'a code hash is 0x and 64 hex digits.';

function hasKnownScriptName(name: string | null | undefined): boolean {
  return Boolean(name && name.trim() && name.trim().toLowerCase() !== UNKNOWN_SCRIPT_NAME);
}

function isHexScriptHash(value: string): boolean {
  return /^0x[0-9a-fA-F]{64}$/.test(value);
}

function normalizeScriptKind(value: string | null | undefined): 'lock' | 'type' | 'both' | null {
  if (value === 'lock' || value === 'type' || value === 'both') return value;
  if (value === 'lock+type') return 'both';
  return null;
}

/** The hash types a script can have on chain. */
const HASH_TYPES = ['type', 'data', 'data1', 'data2'] as const;

/**
 * The hash type asked, or null when none is: a script is its code hash and its hash type, and the
 * registry names the pair (lane rd/api-lock-names), so the page reads the pair asked and never
 * guesses one. Any other word is not a CKB hash type, and the page says so.
 */
function normalizeHashType(value: string | null | undefined): string | null {
  if (!value) return null;
  const normalized = value.trim().toLowerCase();
  if (!normalized) return null;
  if (!(HASH_TYPES as readonly string[]).includes(normalized))
    throw new Error(`/script: hash type ${JSON.stringify(value)} is not a CKB hash type`);
  return normalized;
}

/**
 * A code hash alone names no script: the pairs on it that the registry names or the chain has
 * used, each a link to its own page, and no guess among them.
 */
function CodeHashPairs({
  codeHash,
  pairs,
}: {
  codeHash: string;
  pairs: ScriptLookupInfo[] | undefined;
}) {
  return (
    <div className="min-h-screen">
      <main className="container mx-auto space-y-3 py-8">
        <p className="text-body text-text max-w-measure">
          a code hash alone names no script: a script is its code hash and its hash type.
        </p>
        {pairs === undefined ? (
          <p className="text-text-dim text-body">Reading the scripts on this code hash…</p>
        ) : pairs.length === 0 ? (
          <p className="text-text-dim text-body">no script on this code hash is indexed.</p>
        ) : (
          <ul className="text-body space-y-1">
            {pairs.map((pair) => (
              <li key={pair.hashType}>
                <Link
                  look="inline"
                  href={getScriptDetailHref({ codeHash, hashType: pair.hashType })}
                >
                  by {pair.hashType}
                </Link>
                {pair.name.toLowerCase() !== 'unknown' && (
                  <span className="text-text-dim"> · {pair.name}</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </main>
    </div>
  );
}

export interface ScriptByCodeHashPageProps {
  codeHash: string;
}

export default function ScriptByCodeHashPage({
  codeHash: routeCodeHash,
}: ScriptByCodeHashPageProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const rawIdentifier = decodeURIComponent(routeCodeHash);
  const scriptIdentifier = rawIdentifier.trim();
  const isCodeHashIdentifier = isHexScriptHash(scriptIdentifier);
  // `/script/<name>` is an alias of the named page; `0x…` that is not 32 bytes is neither a code
  // hash nor a name.
  const isMalformedCodeHash = !isCodeHashIdentifier && /^0x/i.test(scriptIdentifier);
  const normalizedCodeHash = isCodeHashIdentifier
    ? `0x${scriptIdentifier.slice(2).toLowerCase()}`
    : scriptIdentifier;
  const initialHashType = normalizeHashType(searchParams.get('hashType'));
  const initialKind = normalizeScriptKind(searchParams.get('kind'));

  useEffect(() => {
    if (!scriptIdentifier || isCodeHashIdentifier || isMalformedCodeHash) {
      return;
    }

    router.replace(`/scripts/${encodeURIComponent(scriptIdentifier)}`);
  }, [isCodeHashIdentifier, isMalformedCodeHash, router, scriptIdentifier]);

  // The pair asked: the reference page's own read of it (its key and its shape, one fetch).
  const { data: knownScript } = useQuery({
    queryKey: ['script-lookup-detail', normalizedCodeHash, initialHashType],
    queryFn: async () => {
      const pair = { codeHash: normalizedCodeHash, hashType: initialHashType! };
      const result = await api.lookupScripts([pair]);
      return result[scriptLookupKey(pair.codeHash, pair.hashType)] ?? null;
    },
    enabled: isCodeHashIdentifier && initialHashType !== null,
    staleTime: Infinity,
  });
  // With no hash type asked, every pair on the code hash: the page lists the ones that exist and
  // guesses none.
  const { data: codeHashPairs } = useQuery({
    queryKey: ['script-code-hash-pairs', normalizedCodeHash],
    queryFn: async () => {
      const result = await api.lookupScripts(
        HASH_TYPES.map((hashType) => ({ codeHash: normalizedCodeHash, hashType }))
      );
      return HASH_TYPES.map(
        (hashType) => result[scriptLookupKey(normalizedCodeHash, hashType)]
      ).filter((entry): entry is ScriptLookupInfo => entry !== undefined);
    },
    enabled: isCodeHashIdentifier && initialHashType === null,
    staleTime: Infinity,
  });
  // the family the registry names for the pair asked
  const family = knownScript && hasKnownScriptName(knownScript.name) ? knownScript : null;

  useEffect(() => {
    if (!isCodeHashIdentifier || !family || family.resolutionState !== 'resolved') {
      return;
    }

    const targetName = family.name.trim();
    const query = new URLSearchParams();
    query.set('ref', normalizedCodeHash);
    if (initialHashType) query.set('hashType', initialHashType);
    const redirectKind = initialKind ?? normalizeScriptKind(family.scriptKind);
    if (redirectKind) query.set('kind', redirectKind);
    const suffix = query.toString();

    router.replace(`/scripts/${encodeURIComponent(targetName)}${suffix ? `?${suffix}` : ''}`);
  }, [initialHashType, initialKind, isCodeHashIdentifier, family, normalizedCodeHash, router]);

  if (isMalformedCodeHash) {
    return <NotFoundPage notAnId={{ kind: 'script', reason: CODE_HASH_RULE }} />;
  }

  if (isCodeHashIdentifier && !initialHashType) {
    return <CodeHashPairs codeHash={normalizedCodeHash} pairs={codeHashPairs} />;
  }

  if (!isCodeHashIdentifier || family) {
    return (
      <div className="min-h-screen">
        <main className="container mx-auto py-8">
          <div className="text-text-dim text-body">Resolving script page…</div>
        </main>
      </div>
    );
  }

  return <ScriptDetailPage codeHash={normalizedCodeHash} />;
}
