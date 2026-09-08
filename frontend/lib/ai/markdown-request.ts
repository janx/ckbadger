import { matchPageRoute } from '@/lib/ai/page-registry';

const MARKDOWN_INTERNAL_PREFIX = '/ai-md';
const RAW_INTERNAL_PREFIX = '/ai-raw';
const RAW_ACCEPT = 'application/vnd.ckbadger.raw+json';

export class FormatRequestError extends Error {
  status = 400;
  code = 'invalid_format';
}

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
  internalPrefix?: '/ai-md' | '/ai-raw';
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
  return last.includes('.') && !last.startsWith('.');
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
  return ['/api', '/ws', '/assets', '/_next', MARKDOWN_INTERNAL_PREFIX, RAW_INTERNAL_PREFIX].some(
    (prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`)
  );
}

function acceptFormat(header: string | null): 'html' | 'md' | 'raw' {
  const candidates: { format: 'html' | 'md' | 'raw'; quality: number }[] = [];
  for (const entry of (header ?? '').split(',')) {
    const [media, ...parameters] = entry.trim().toLowerCase().split(';');
    const format =
      media === RAW_ACCEPT
        ? 'raw'
        : media === 'text/markdown'
          ? 'md'
          : ['text/html', 'application/xhtml+xml', '*/*', 'text/*'].includes(media)
            ? 'html'
            : null;
    if (!format) continue;
    const weight = parameters.find((param) => param.trim().startsWith('q='));
    const quality = weight === undefined ? 1 : Number(weight.trim().slice(2));
    if (!Number.isFinite(quality) || quality < 0 || quality > 1) {
      throw new FormatRequestError(`Invalid Accept quality: ${entry}`);
    }
    if (quality > 0) candidates.push({ format, quality });
  }
  candidates.sort(
    (a, b) => b.quality - a.quality || (a.format === 'html' ? -1 : b.format === 'html' ? 1 : 0)
  );
  return candidates[0]?.format ?? 'html';
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
  if (
    !hasKnownSuffix &&
    hasFileExtension(pathname) &&
    matchPageRoute(pathname).kind === 'unknown' &&
    (pathname.split('/').length <= 2 ||
      matchPageRoute(`/${pathname.split('/').slice(2).join('/')}`).kind === 'unknown')
  ) {
    return { rewrite: false };
  }

  const formats = input.searchParams.getAll('format');
  if (formats.length > 1 || (formats.length === 1 && !['html', 'md', 'raw'].includes(formats[0]))) {
    throw new FormatRequestError(`Invalid query format: ${formats.join(',')}`);
  }
  const format = formats[0];
  if (format === 'html') {
    return {
      rewrite: false,
      sourcePath: stripKnownFormatSuffix(pathname),
      removeFormatParam: true,
    };
  }
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

  const accepted = acceptFormat(input.acceptHeader);
  if (accepted === 'raw') {
    return { rewrite: true, sourcePath: pathname, internalPrefix: RAW_INTERNAL_PREFIX };
  }

  if (accepted === 'md') {
    return { rewrite: true, sourcePath: pathname, internalPrefix: MARKDOWN_INTERNAL_PREFIX };
  }

  return { rewrite: false };
}
