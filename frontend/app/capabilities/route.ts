import type { NextRequest } from 'next/server';
import { buildAiCapabilities } from '@/lib/ai/capabilities';

export const runtime = 'nodejs';

export async function GET(request: NextRequest): Promise<Response> {
  const body = buildAiCapabilities(request.nextUrl.origin);
  return new Response(JSON.stringify(body, null, 2), {
    status: 200,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'public, s-maxage=10, stale-while-revalidate=30',
    },
  });
}
