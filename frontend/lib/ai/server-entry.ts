// Bundled for the embedded QuickJS runtime; never loaded by the SPA.
import 'url-search-params-polyfill';
import { Buffer } from 'buffer';
import { ApiRequestError } from '@/lib/api';
import { buildAiCapabilities } from '@/lib/ai/capabilities';
import { FormatRequestError, resolveMarkdownRewrite } from '@/lib/ai/markdown-request';
import { CHART_PAGE_SLUGS, parseMarkdownSourcePath } from '@/lib/ai/markdown-route';
import { MarkdownRenderError, renderMarkdownPage } from '@/lib/ai/markdown-renderer';
import { parseRawSourcePath } from '@/lib/ai/raw-route';
import { RawRenderError, renderRawPage } from '@/lib/ai/raw-renderer';
import { PAGE_ROUTES } from '@/lib/ai/page-registry';
import type { CkbadgerRuntimeConfig } from '@/lib/runtime-config';

interface ServerInput {
  pathname: string;
  query: string;
  accept: string | null;
  method: string;
  origin: string;
  runtimeConfig: CkbadgerRuntimeConfig;
}

interface ServerOutput {
  status: number;
  body: string;
  headers: Record<string, string>;
  htmlPath?: string;
}

declare const __ckbadgerFetch: (url: string, method: string, body: string) => Promise<string>;

function configure(input: ServerInput) {
  // A fresh JS context per request keeps active network and metadata isolated.
  Object.assign(globalThis, {
    Buffer,
    window: {
      location: { pathname: input.pathname, origin: input.origin },
      __CKBADGER_RUNTIME_CONFIG__: input.runtimeConfig,
    },
    fetch: async (url: string, init?: RequestInit) => {
      const response = JSON.parse(
        await __ckbadgerFetch(url, init?.method ?? 'GET', String(init?.body ?? ''))
      ) as { status: number; body: string };
      return {
        ok: response.status >= 200 && response.status < 300,
        status: response.status,
        json: async () => JSON.parse(response.body),
      };
    },
  });
}

export function discovery(serialized: string): string {
  const input: ServerInput = JSON.parse(serialized);
  configure(input);
  return JSON.stringify(buildAiCapabilities(input.origin));
}

export function routeDocumentation(): string {
  return [
    '## Registered page formats',
    '',
    'Paths below are relative to `/{network}`. Read `/capabilities` for enabled networks.',
    'Negotiation: `?format=html|md|raw` > `.md|.raw` > `Accept` (quality weights respected).',
    'Markdown: `text/markdown`; raw: `application/vnd.ckbadger.raw+json`.',
    'Raw defaults to `profile=default`. Invalid profiles and unsupported routes return JSON 4xx.',
    'Responses vary on `Accept` and use `Cache-Control: no-store`.',
    '',
    '| Page | Markdown | Raw profiles |',
    '| --- | --- | --- |',
    ...PAGE_ROUTES.map(
      (route) => `| \`${route.pattern}\` | .md | ${route.rawProfiles.join(', ') || '—'} |`
    ),
    '',
    `Chart slugs: ${CHART_PAGE_SLUGS.map((slug) => `\`${slug}\``).join(', ')}.`,
    '',
  ].join('\n');
}

export async function render(serialized: string): Promise<string> {
  const input: ServerInput = JSON.parse(serialized);
  configure(input);
  const headers: Record<string, string> = { vary: 'Accept', 'cache-control': 'no-store' };
  const output: ServerOutput = { status: 200, body: '', headers };
  try {
    const params = new URLSearchParams(input.query);
    const decision = resolveMarkdownRewrite({
      method: input.method,
      pathname: input.pathname,
      searchParams: params,
      acceptHeader: input.accept,
    });
    const fullPath = decision.sourcePath ?? input.pathname;
    if (!decision.rewrite) {
      output.htmlPath = fullPath;
      return JSON.stringify(output);
    }
    const [network, ...segments] = fullPath.replace(/^\//, '').split('/');
    if (!input.runtimeConfig.networks?.some((item) => item.name === network)) {
      throw new RawRenderError(404, 'unknown_network', `Unknown network "${network}"`);
    }
    window.location.pathname = fullPath;
    params.delete('format');
    const sourcePath = `/${segments.join('/')}`;
    if (decision.internalPrefix === '/ai-raw') {
      const page = parseRawSourcePath(sourcePath);
      if (page.kind === 'unknown') {
        throw new RawRenderError(404, 'unknown_page', `No raw renderer for "${fullPath}"`);
      }
      // The route parser sees a network-relative path; metadata uses the full URL.
      page.pathname = fullPath;
      const rendered = await renderRawPage({ page, searchParams: params, origin: input.origin });
      output.status = rendered.status;
      output.body = JSON.stringify(rendered.body);
      headers['content-type'] = 'application/vnd.ckbadger.raw+json; charset=utf-8';
      headers['x-ckbadger-format'] = 'raw';
      headers['x-ckbadger-profile'] = rendered.body.meta.profile;
      headers['x-ckbadger-schema'] = rendered.body.meta.schemaVersion;
    } else {
      const page = parseMarkdownSourcePath(sourcePath);
      if (page.kind === 'unknown') {
        throw new RawRenderError(404, 'unknown_page', `No markdown renderer for "${fullPath}"`);
      }
      // Some static routes have literal pathname types; the runtime path adds network.
      Object.assign(page, { pathname: fullPath });
      const rendered = await renderMarkdownPage({
        page,
        searchParams: params,
        origin: input.origin,
      });
      output.status = rendered.status;
      output.body = rendered.body;
      headers['content-type'] = 'text/markdown; charset=utf-8';
      headers['x-ckbadger-format'] = 'md';
    }
  } catch (error) {
    output.status =
      error instanceof RawRenderError ||
      error instanceof MarkdownRenderError ||
      error instanceof ApiRequestError ||
      error instanceof FormatRequestError
        ? error.status
        : error instanceof URIError
          ? 400
          : 500;
    const code =
      error instanceof RawRenderError ||
      error instanceof ApiRequestError ||
      error instanceof FormatRequestError
        ? error.code
        : output.status === 400
          ? 'invalid_request'
          : 'render_error';
    output.body = JSON.stringify({
      error: {
        code,
        message: error instanceof Error ? error.message : String(error),
        path: input.pathname,
        ...(error instanceof RawRenderError && error.details ? { details: error.details } : {}),
      },
    });
    headers['content-type'] = 'application/json; charset=utf-8';
  }
  return JSON.stringify(output);
}
