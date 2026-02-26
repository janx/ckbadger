import type { NextRequest } from 'next/server';
import { parseRawSourcePath } from '@/lib/ai/raw-route';
import { buildRawErrorResponse, RawRenderError, renderRawPage } from '@/lib/ai/raw-renderer';

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

function jsonResponse(status: number, body: unknown): Response {
  const meta = (body as { meta?: { profile?: string; schemaVersion?: string } })?.meta;
  const profile = meta?.profile;
  const schemaVersion = meta?.schemaVersion;
  return new Response(JSON.stringify(body, null, 2), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control':
        status === 200 ? 'public, s-maxage=10, stale-while-revalidate=30' : 'no-store',
      'x-ckbadger-format': 'raw',
      ...(profile ? { 'x-ckbadger-profile': profile } : {}),
      ...(schemaVersion ? { 'x-ckbadger-schema': schemaVersion } : {}),
    },
  });
}

export async function GET(request: NextRequest, context: RouteContext): Promise<Response> {
  const { slug } = await context.params;
  const sourcePath = toSourcePath(slug);
  const parsed = parseRawSourcePath(sourcePath);
  const origin = request.nextUrl.origin;
  const profile = request.nextUrl.searchParams.get('profile');

  try {
    const result = await renderRawPage({
      page: parsed,
      searchParams: request.nextUrl.searchParams,
      origin,
    });
    return jsonResponse(result.status, result.body);
  } catch (error) {
    if (error instanceof RawRenderError) {
      const result = buildRawErrorResponse(sourcePath, profile, origin, parsed.kind, error);
      return jsonResponse(result.status, result.body);
    }

    const unexpected = new RawRenderError(
      502,
      'upstream_error',
      error instanceof Error ? error.message : String(error)
    );
    const result = buildRawErrorResponse(sourcePath, profile, origin, parsed.kind, unexpected);
    return jsonResponse(result.status, result.body);
  }
}
