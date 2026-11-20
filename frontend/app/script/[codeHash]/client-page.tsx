'use client';

import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useRouter, useSearchParams } from '@/src/navigation';
import { api } from '@/lib/api';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { NotFoundPage } from '@/components/not-found-page';

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

function normalizeHashType(value: string | null | undefined): string | null {
  if (!value) return null;
  const normalized = value.trim().toLowerCase();
  return normalized || null;
}

/**
 * Whether the family the registry names for a code hash references it by the hash type asked
 * (lane B4 third round): by `type` where the code hash is the family's type id (or the registry
 * references it by type), by `data` / `data1` / `data2` where it is the code's data hash (or the
 * registry references it by data). The registry names a code hash alone, so a lock on a family's
 * type-id code hash by data is another script, no cell's data hashing to a type id: it stays on
 * the code hash, read by the hash type asked. No hash type asked takes the registry's reference.
 */
function familyReferences(
  known: {
    hashType: string | null;
    deploymentTypeHash?: string | null;
    deploymentDataHash?: string | null;
  },
  codeHash: string,
  hashType: string | null
): boolean {
  if (hashType === null) return true;
  if (hashType === 'type')
    return known.hashType === 'type' || known.deploymentTypeHash?.toLowerCase() === codeHash;
  if (hashType === 'data' || hashType === 'data1' || hashType === 'data2')
    return (
      (known.hashType !== null && known.hashType !== 'type') ||
      known.deploymentDataHash?.toLowerCase() === codeHash
    );
  throw new Error(
    `/script/${codeHash}: hash type ${JSON.stringify(hashType)} is not a CKB hash type`
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

  const { data: lookupResult } = useQuery({
    queryKey: ['script-lookup-detail', normalizedCodeHash],
    queryFn: async () => {
      const result = await api.lookupScripts([normalizedCodeHash]);
      return result[normalizedCodeHash] ?? null;
    },
    enabled: isCodeHashIdentifier,
    staleTime: Infinity,
  });
  const knownScript = lookupResult;
  // the family the registry names, where it references this code hash by the hash type asked
  const family =
    knownScript &&
    hasKnownScriptName(knownScript.name) &&
    familyReferences(knownScript, normalizedCodeHash, initialHashType)
      ? knownScript
      : null;

  useEffect(() => {
    if (!isCodeHashIdentifier || !family || family.resolutionState !== 'resolved') {
      return;
    }

    const targetName = family.name.trim();
    const query = new URLSearchParams();
    query.set('ref', normalizedCodeHash);
    const redirectHashType = initialHashType ?? normalizeHashType(family.hashType);
    if (redirectHashType) query.set('hashType', redirectHashType);
    const redirectKind = initialKind ?? normalizeScriptKind(family.scriptKind);
    if (redirectKind) query.set('kind', redirectKind);
    const suffix = query.toString();

    router.replace(`/scripts/${encodeURIComponent(targetName)}${suffix ? `?${suffix}` : ''}`);
  }, [initialHashType, initialKind, isCodeHashIdentifier, family, normalizedCodeHash, router]);

  if (isMalformedCodeHash) {
    return <NotFoundPage notAnId={{ kind: 'script', reason: CODE_HASH_RULE }} />;
  }

  if (!isCodeHashIdentifier || family) {
    return (
      <div className="min-h-screen">
        <main className="container mx-auto py-8">
          <div className="text-text text-body">Resolving script page…</div>
        </main>
      </div>
    );
  }

  return <ScriptDetailPage codeHash={normalizedCodeHash} />;
}
