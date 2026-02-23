const INTERNAL_PREFIX = '/__md';

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

function isMethodAllowed(method: string): boolean {
  return method === 'GET' || method === 'HEAD';
}

function isInternalOrApiPath(pathname: string): boolean {
  return (
    pathname.startsWith('/api') ||
    pathname.startsWith('/_next') ||
    pathname.startsWith(INTERNAL_PREFIX)
  );
}

export function resolveMarkdownRewrite(input: MarkdownRewriteInput): MarkdownRewriteDecision {
  if (!isMethodAllowed(input.method)) {
    return { rewrite: false };
  }

  const pathname = normalizePathname(input.pathname);

  if (pathname.endsWith('.md')) {
    const sourcePath = pathname.slice(0, -3) || '/';
    return { rewrite: true, sourcePath };
  }

  if (isInternalOrApiPath(pathname)) {
    return { rewrite: false };
  }

  if (hasFileExtension(pathname)) {
    return { rewrite: false };
  }

  const format = input.searchParams.get('format');
  if (format === 'md') {
    return { rewrite: true, sourcePath: pathname, removeFormatParam: true };
  }

  if (input.acceptHeader?.includes('text/markdown')) {
    return { rewrite: true, sourcePath: pathname };
  }

  return { rewrite: false };
}
