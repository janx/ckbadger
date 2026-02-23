import type { NextRequest } from 'next/server';
import { NextResponse } from 'next/server';
import { resolveMarkdownRewrite } from '@/lib/ai/markdown-request';

const MARKDOWN_INTERNAL_PREFIX = '/__md';
const RAW_INTERNAL_PREFIX = '/__raw';

export function middleware(request: NextRequest): NextResponse {
  const decision = resolveMarkdownRewrite({
    method: request.method,
    pathname: request.nextUrl.pathname,
    searchParams: request.nextUrl.searchParams,
    acceptHeader: request.headers.get('accept'),
  });

  if (!decision.rewrite || !decision.sourcePath) {
    return NextResponse.next();
  }

  const rewriteUrl = request.nextUrl.clone();
  const internalPrefix = decision.internalPrefix ?? MARKDOWN_INTERNAL_PREFIX;
  if (internalPrefix !== MARKDOWN_INTERNAL_PREFIX && internalPrefix !== RAW_INTERNAL_PREFIX) {
    return NextResponse.next();
  }
  rewriteUrl.pathname = `${internalPrefix}${
    decision.sourcePath === '/' ? '' : decision.sourcePath
  }`;
  if (decision.removeFormatParam) {
    rewriteUrl.searchParams.delete('format');
  }
  return NextResponse.rewrite(rewriteUrl);
}

export const config = {
  matcher: '/:path*',
};
