import type { NextRequest } from 'next/server';
import { buildMarkdownDocument } from '@/lib/ai/markdown-format';
import { parseMarkdownSourcePath } from '@/lib/ai/markdown-route';
import { MarkdownRenderError, renderMarkdownPage } from '@/lib/ai/markdown-renderer';

export const runtime = 'nodejs';

interface RouteContext {
  params: Promise<{
    slug?: string[];
  }>;
}

function toSourcePath(slug: string[] | undefined): string {
  if (!slug || slug.length === 0) return '/';
  const normalizedSegments = slug.map((segment) => {
    try {
      return decodeURIComponent(segment);
    } catch {
      return segment;
    }
  });
  return `/${normalizedSegments.join('/')}`;
}

function markdownResponse(status: number, body: string): Response {
  return new Response(body, {
    status,
    headers: {
      'content-type': 'text/markdown; charset=utf-8',
      'cache-control':
        status === 200 ? 'public, s-maxage=10, stale-while-revalidate=30' : 'no-store',
    },
  });
}

export async function GET(request: NextRequest, context: RouteContext): Promise<Response> {
  const { slug } = await context.params;
  const sourcePath = toSourcePath(slug);
  const parsed = parseMarkdownSourcePath(sourcePath);
  const origin = request.nextUrl.origin;

  try {
    const result = await renderMarkdownPage({
      page: parsed,
      searchParams: request.nextUrl.searchParams,
      origin,
    });
    return markdownResponse(result.status, result.body);
  } catch (error) {
    if (error instanceof MarkdownRenderError) {
      const body = buildMarkdownDocument(
        {
          title: `ckbadger markdown error - ${sourcePath}`,
          path: sourcePath,
          canonical: `${origin}${sourcePath}`,
          pageType: parsed.kind,
          generatedAt: new Date().toISOString(),
        },
        ['# Markdown Render Error', '', error.message]
      );
      return markdownResponse(error.status, body);
    }

    const message = error instanceof Error ? error.message : String(error);
    const body = buildMarkdownDocument(
      {
        title: `ckbadger markdown upstream error - ${sourcePath}`,
        path: sourcePath,
        canonical: `${origin}${sourcePath}`,
        pageType: parsed.kind,
        generatedAt: new Date().toISOString(),
      },
      ['# Upstream Error', '', message]
    );
    return markdownResponse(502, body);
  }
}
