import { useEffect, useState } from 'react';
import { api, type SearchResponse } from '@/lib/api';
import { resolveActiveNetwork } from '@/lib/active-network';

export type EntityKind =
  | 'block'
  | 'transaction'
  | 'cell'
  | 'address'
  | 'script'
  | 'script-family'
  | 'token'
  | 'object'
  | 'collection'
  | 'identity'
  | 'class'
  | 'cluster'
  | 'fork'
  | 'fiber-channel';

export interface NotFoundEntity {
  kind: EntityKind;
  identifier: string;
  /** Exact relative API path used by the owning entity query that returned 404. */
  checkedEndpoint: string;
}

export interface NotFoundEvidence {
  /** The entity request that returned HTTP 404. This is the only absence claim. */
  checkedEndpoint: string;
  /** A separate, scoped classifier lookup; null where no exact lookup is suitable. */
  query: string | null;
  searchEndpoint: string | null;
  finding: string;
  alternative?: {
    label: string;
    href: string;
    resultType: 'block' | 'transaction';
    matchKind: 'exact_hash';
  };
}

const HASH = /^0x[0-9a-fA-F]{64}$/;
const OUTPOINT = /^(0x[0-9a-fA-F]{64})-(0|[1-9][0-9]*)$/;

function checkedPath(entity: NotFoundEntity): string {
  const path = entity.checkedEndpoint;
  if (!/^\/(?!\/)[^#]*$/.test(path) || path.split('?')[0].split('/').includes('..')) {
    throw new Error(`Invalid entity evidence endpoint: ${path}`);
  }
  return path;
}

type ExactSearchKind = 'block' | 'transaction' | 'cell';

function exactSearch(entity: NotFoundEntity): { kind: ExactSearchKind; query: string } | null {
  const id = entity.identifier;
  if (entity.kind === 'block' && HASH.test(id)) return { kind: 'transaction', query: `tx:${id}` };
  if (entity.kind === 'transaction' && HASH.test(id))
    return { kind: 'block', query: `block:${id}` };
  if (entity.kind === 'cell' && OUTPOINT.test(id)) return { kind: 'cell', query: `cell:${id}` };
  return null;
}

/** Reuses /search's typed classifier, with exact form and URL validation before linking. */
export async function resolveNotFoundEvidence(
  entity: NotFoundEntity,
  search: (query: string) => Promise<SearchResponse> = api.search
): Promise<NotFoundEvidence> {
  const checkedEndpoint = checkedPath(entity);
  const plan = exactSearch(entity);
  if (!plan) {
    return {
      checkedEndpoint,
      query: null,
      searchEndpoint: null,
      finding: `The ${checkedEndpoint} entity request returned HTTP 404. No exact identifier search was run.`,
    };
  }
  const response = await search(plan.query);
  if (response.query !== plan.query || !Array.isArray(response.results)) {
    throw new Error(`Search response for ${plan.query} did not match the requested query`);
  }
  const base = {
    checkedEndpoint,
    query: plan.query,
    searchEndpoint: `/search?q=${encodeURIComponent(plan.query)}`,
  };
  if (plan.kind === 'cell') {
    const outpoint = entity.identifier.toLowerCase();
    const hit = response.results.find(
      (result) =>
        result.resultType === 'cell' &&
        result.matchKind === 'outpoint' &&
        result.id.toLowerCase() === outpoint &&
        result.url.toLowerCase() === `/cell/${outpoint}`
    );
    if (hit) {
      return {
        ...base,
        finding: `The /search cell lookup returned “${hit.label}” for ${outpoint}, while ${checkedEndpoint} returned HTTP 404. These results disagree; the 404 does not prove the cell never existed.`,
      };
    }
  } else {
    const hash = entity.identifier.toLowerCase();
    const url = plan.kind === 'block' ? `/blocks/${hash}` : `/tx/${hash}`;
    const hit = response.results.find(
      (result) =>
        result.resultType === plan.kind &&
        result.matchKind === 'exact_hash' &&
        result.url.toLowerCase() === url &&
        (plan.kind === 'block'
          ? /^(0|[1-9][0-9]*)$/.test(result.id)
          : result.id.toLowerCase() === hash)
    );
    if (hit) {
      return {
        ...base,
        finding: `The ${checkedEndpoint} entity request returned HTTP 404; /search identified this exact hash as a ${plan.kind}.`,
        alternative: {
          label: hit.label,
          href: hit.url,
          resultType: plan.kind,
          matchKind: 'exact_hash',
        },
      };
    }
  }
  return {
    ...base,
    finding: `The ${checkedEndpoint} entity request returned HTTP 404; /search returned no typed match for ${plan.query}.`,
  };
}

export type NotFoundEvidenceState =
  | { status: 'idle' | 'loading'; evidence?: never; error?: never }
  | { status: 'checked'; evidence: NotFoundEvidence; error?: never }
  | { status: 'error'; evidence?: never; error: string };

/** Provider-independent because route error elements can render outside QueryClientProvider. */
export function useNotFoundEvidence(entity?: NotFoundEntity): NotFoundEvidenceState {
  const network = resolveActiveNetwork();
  const key = entity
    ? JSON.stringify([network, entity.kind, entity.identifier, entity.checkedEndpoint])
    : null;
  const [state, setState] = useState<{ key: string | null; value: NotFoundEvidenceState }>({
    key: null,
    value: { status: 'idle' },
  });
  useEffect(() => {
    if (!entity) {
      setState({ key: null, value: { status: 'idle' } });
      return;
    }
    let current = true;
    setState({ key, value: { status: 'loading' } });
    resolveNotFoundEvidence(entity)
      .then((evidence) => {
        if (current) setState({ key, value: { status: 'checked', evidence } });
      })
      .catch((error: unknown) => {
        if (current) setState({ key, value: { status: 'error', error: String(error) } });
      });
    return () => {
      current = false;
    };
  }, [key]);
  return state.key === key ? state.value : { status: key === null ? 'idle' : 'loading' };
}
