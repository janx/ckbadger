import { defineConfig, type Plugin, type ProxyOptions, type ViteDevServer } from 'vite';
import { request as httpRequest } from 'node:http';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export const SHARED_FRONTEND_ORIGIN = 'http://127.0.0.1:8100';

export const DEVELOPMENT_PROXY = {
  '/runtime-config.js': SHARED_FRONTEND_ORIGIN,
  '/capabilities': SHARED_FRONTEND_ORIGIN,
  '/llms.txt': SHARED_FRONTEND_ORIGIN,
  '/llms-full.txt': SHARED_FRONTEND_ORIGIN,
  '/api': SHARED_FRONTEND_ORIGIN,
  '/ws': {
    target: SHARED_FRONTEND_ORIGIN,
    ws: true,
  },
} satisfies Record<string, string | ProxyOptions>;

/** Vite delegates negotiated pages to the same Axum renderer used by releases. */
export function agentFormatProxy(origin: string): Plugin {
  const install = (server: Pick<ViteDevServer, 'middlewares'>) => {
    server.middlewares.use((request, response, next) => {
      const url = new URL(request.url ?? '/', origin);
      // Candidate detection only; Axum runs the single negotiation implementation.
      const path = url.pathname.replace(/\/+$/, '');
      const accept = (request.headers.accept ?? '').toLowerCase();
      const candidate =
        path.endsWith('.md') ||
        path.endsWith('.raw') ||
        url.searchParams.has('format') ||
        accept.includes('text/markdown') ||
        accept.includes('application/vnd.ckbadger.raw+json');
      if (!candidate || /^\/(api|ws|assets)(\/|$)/.test(path)) return next();
      const upstream = httpRequest(
        new URL(`${url.pathname}${url.search}`, origin),
        {
          method: request.method,
          headers: request.headers,
        },
        (reply) => {
          response.writeHead(reply.statusCode ?? 502, reply.headers);
          reply.pipe(response);
        }
      );
      upstream.on('error', (error) => {
        response.writeHead(502, {
          'content-type': 'application/json',
          'cache-control': 'no-store',
          vary: 'Accept',
        });
        response.end(
          JSON.stringify({ error: { code: 'upstream_unreachable', message: error.message } })
        );
      });
      request.pipe(upstream);
    });
  };
  return {
    name: 'ckbadger-agent-formats',
    configureServer: install,
    configurePreviewServer: install,
  };
}

export default defineConfig({
  plugins: [react(), agentFormatProxy(SHARED_FRONTEND_ORIGIN)],
  server: {
    proxy: DEVELOPMENT_PROXY,
  },
  preview: { proxy: DEVELOPMENT_PROXY },
  resolve: {
    alias: {
      '@': resolve(__dirname, '.'),
    },
  },
});
