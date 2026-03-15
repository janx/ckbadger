'use client';

import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useRouter, useSearchParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { api } from '@/lib/api';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';

const UNKNOWN_SCRIPT_NAME = 'unknown';

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
  const normalizedCodeHash = isCodeHashIdentifier
    ? `0x${scriptIdentifier.slice(2).toLowerCase()}`
    : scriptIdentifier;
  const initialHashType = normalizeHashType(searchParams.get('hashType'));
  const initialKind = normalizeScriptKind(searchParams.get('kind'));

  useEffect(() => {
    if (!scriptIdentifier || isCodeHashIdentifier) {
      return;
    }

    router.replace(`/scripts/${encodeURIComponent(scriptIdentifier)}`);
  }, [isCodeHashIdentifier, router, scriptIdentifier]);

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

  useEffect(() => {
    if (!isCodeHashIdentifier || !knownScript || !hasKnownScriptName(knownScript.name)) {
      return;
    }

    const targetName = knownScript.name.trim();
    const query = new URLSearchParams();
    query.set('ref', normalizedCodeHash);
    const redirectHashType = initialHashType ?? normalizeHashType(knownScript.hashType);
    if (redirectHashType) query.set('hashType', redirectHashType);
    const redirectKind = initialKind ?? normalizeScriptKind(knownScript.scriptKind);
    if (redirectKind) query.set('kind', redirectKind);
    const suffix = query.toString();

    router.replace(`/scripts/${encodeURIComponent(targetName)}${suffix ? `?${suffix}` : ''}`);
  }, [initialHashType, initialKind, isCodeHashIdentifier, knownScript, normalizedCodeHash, router]);

  if (!isCodeHashIdentifier || (knownScript && hasKnownScriptName(knownScript.name))) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="text-text-dim text-sm">Resolving script page...</div>
        </main>
      </div>
    );
  }

  return <ScriptDetailPage codeHash={normalizedCodeHash} />;
}
