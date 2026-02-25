import {
  isCkbAddress,
  normalizeHash32,
  parseOutpoint,
  parseSearchIntent,
} from '@/lib/search-intent';

export function resolveSearchRoute(input: string): string | null {
  const intent = parseSearchIntent(input);
  const body = intent.body.trim();

  if (!body) return null;

  if (intent.prefix === 'block') {
    if (/^[0-9]+$/.test(body)) return `/blocks/${body}`;
    const hash = normalizeHash32(body);
    return hash ? `/blocks/${hash}` : null;
  }

  if (intent.prefix === 'tx') {
    const hash = normalizeHash32(body);
    return hash ? `/tx/${hash}` : null;
  }

  if (intent.prefix === 'address') {
    if (isCkbAddress(body)) return `/address/${body}`;
    const lockHash = normalizeHash32(body);
    return lockHash ? `/address/${lockHash}` : null;
  }

  if (intent.prefix === 'cell') {
    const outpoint = parseOutpoint(body);
    return outpoint ? `/cell/${outpoint.normalized}` : null;
  }

  if (intent.prefix === 'script') {
    const hash = normalizeHash32(body);
    return hash ? `/script/${hash}` : null;
  }

  if (intent.prefix === 'token') {
    const hash = normalizeHash32(body);
    return hash ? `/tokens/${hash}` : null;
  }

  if (intent.prefix === 'spore') {
    const hash = normalizeHash32(body);
    return hash ? `/nfts/${hash}` : null;
  }

  if (intent.prefix === 'cluster') {
    const hash = normalizeHash32(body);
    return hash ? `/clusters/${hash}` : null;
  }

  if (/^[0-9]+$/.test(body)) {
    return `/blocks/${body}`;
  }

  if (isCkbAddress(body)) {
    return `/address/${body}`;
  }

  const outpoint = parseOutpoint(body);
  if (outpoint) {
    return `/cell/${outpoint.normalized}`;
  }

  return null;
}
