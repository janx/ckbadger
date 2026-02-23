export type ParsedRawPage =
  | { kind: 'block_detail'; pathname: string; id: string }
  | { kind: 'cell_detail'; pathname: string; outpoint: string }
  | { kind: 'tx_detail'; pathname: string; hash: string }
  | { kind: 'unknown'; pathname: string };

export const RAW_ROUTE_PATTERNS = ['/blocks/{id}', '/cell/{outpoint}', '/tx/{hash}'] as const;

function normalizePathname(pathname: string): string {
  if (!pathname.startsWith('/')) {
    return `/${pathname}`;
  }
  if (pathname !== '/' && pathname.endsWith('/')) {
    return pathname.replace(/\/+$/, '');
  }
  return pathname;
}

function decodeParam(raw: string): string {
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

export function parseRawSourcePath(pathname: string): ParsedRawPage {
  const normalized = normalizePathname(pathname);

  const blockMatch = normalized.match(/^\/blocks\/([^/]+)$/);
  if (blockMatch) {
    return {
      kind: 'block_detail',
      pathname: normalized,
      id: decodeParam(blockMatch[1]),
    };
  }

  const cellMatch = normalized.match(/^\/cell\/([^/]+)$/);
  if (cellMatch) {
    return {
      kind: 'cell_detail',
      pathname: normalized,
      outpoint: decodeParam(cellMatch[1]),
    };
  }

  const txMatch = normalized.match(/^\/tx\/([^/]+)$/);
  if (txMatch) {
    return {
      kind: 'tx_detail',
      pathname: normalized,
      hash: decodeParam(txMatch[1]),
    };
  }

  return {
    kind: 'unknown',
    pathname: normalized,
  };
}
