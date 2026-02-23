const MARKDOWN_INTERNAL_PREFIX = '/__md';
const RAW_INTERNAL_PREFIX = '/__raw';
const RAW_ACCEPT = 'application/vnd.ckbadger.raw+json';

interface MarkdownRewriteInput {
  method: string;
  pathname: string;
  searchParams: URLSearchParams;
  acceptHeader: string | null;
}

export interface MarkdownRewriteDecision {
  rewrite: boolean;
  sourcePath?: string;
  removeFormatParam?: boolean;
  internalPrefix?: '/__md' | '/__raw';
}

function normalizePathname(pathname: string): string {
  if (!pathname.startsWith('/')) return `/${pathname}`;
  if (pathname !== '/' && pathname.endsWith('/')) {
    return pathname.replace(/\/+$/, '');
  }
  return pathname;
}

function hasFileExtension(pathname: string): boolean {
  const last = pathname.split('/').pop() ?? '';
  return last.includes('.');
}

function stripKnownFormatSuffix(pathname: string): string {
  if (pathname.endsWith('.md')) {
    return pathname.slice(0, -3) || '/';
  }
  if (pathname.endsWith('.raw')) {
    return pathname.slice(0, -4) || '/';
  }
  return pathname;
}

function isMethodAllowed(method: string): boolean {
  return method === 'GET' || method === 'HEAD';
}

function isInternalOrApiPath(pathname: string): boolean {
  return (
    pathname.startsWith('/api') ||
    pathname.startsWith('/_next') ||
    pathname.startsWith(MARKDOWN_INTERNAL_PREFIX) ||
    pathname.startsWith(RAW_INTERNAL_PREFIX)
  );
}

export function resolveMarkdownRewrite(input: MarkdownRewriteInput): MarkdownRewriteDecision {
  if (!isMethodAllowed(input.method)) {
    return { rewrite: false };
  }

  const pathname = normalizePathname(input.pathname);

  if (isInternalOrApiPath(pathname)) {
    return { rewrite: false };
  }

  const hasKnownSuffix = pathname.endsWith('.md') || pathname.endsWith('.raw');
  if (!hasKnownSuffix && hasFileExtension(pathname)) {
    return { rewrite: false };
  }

  const format = input.searchParams.get('format');
  if (format === 'md') {
    return {
      rewrite: true,
      sourcePath: stripKnownFormatSuffix(pathname),
      removeFormatParam: true,
      internalPrefix: MARKDOWN_INTERNAL_PREFIX,
    };
  }

  if (format === 'raw') {
    return {
      rewrite: true,
      sourcePath: stripKnownFormatSuffix(pathname),
      removeFormatParam: true,
      internalPrefix: RAW_INTERNAL_PREFIX,
    };
  }

  if (pathname.endsWith('.md')) {
    const sourcePath = pathname.slice(0, -3) || '/';
    return { rewrite: true, sourcePath, internalPrefix: MARKDOWN_INTERNAL_PREFIX };
  }

  if (pathname.endsWith('.raw')) {
    const sourcePath = pathname.slice(0, -4) || '/';
    return { rewrite: true, sourcePath, internalPrefix: RAW_INTERNAL_PREFIX };
  }

  if (input.acceptHeader?.includes(RAW_ACCEPT)) {
    return { rewrite: true, sourcePath: pathname, internalPrefix: RAW_INTERNAL_PREFIX };
  }

  if (input.acceptHeader?.includes('text/markdown')) {
    return { rewrite: true, sourcePath: pathname, internalPrefix: MARKDOWN_INTERNAL_PREFIX };
  }

  return { rewrite: false };
}
