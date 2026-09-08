// @vitest-environment node

import { createServer, request } from 'node:http';
import type { AddressInfo } from 'node:net';
import type { Connect, PreviewServer, ProxyOptions, UserConfig, ViteDevServer } from 'vite';
import viteConfig, {
  agentFormatProxy,
  DEVELOPMENT_PROXY,
  SHARED_FRONTEND_ORIGIN,
} from '@/vite.config';

describe('Vite development proxy', () => {
  it('routes runtime configuration and HTTP endpoints through the shared frontend', () => {
    expect(DEVELOPMENT_PROXY['/runtime-config.js']).toBe(SHARED_FRONTEND_ORIGIN);
    expect(DEVELOPMENT_PROXY['/capabilities']).toBe(SHARED_FRONTEND_ORIGIN);
    expect(DEVELOPMENT_PROXY['/api']).toBe(SHARED_FRONTEND_ORIGIN);
  });

  it('routes WebSocket upgrades through the shared frontend', () => {
    const wsProxy = DEVELOPMENT_PROXY['/ws'];

    expect(wsProxy).not.toBeUndefined();
    expect(typeof wsProxy).not.toBe('string');

    const options = wsProxy as ProxyOptions;
    expect(options.target).toBe(SHARED_FRONTEND_ORIGIN);
    expect(options.ws).toBe(true);
    expect(options.changeOrigin).toBeUndefined();
  });

  it('installs the development proxy in the exported Vite config', () => {
    expect(typeof viteConfig).not.toBe('function');

    const config = viteConfig as UserConfig;
    expect(config.server?.proxy).toBe(DEVELOPMENT_PROXY);
    expect(config.preview?.proxy).toBe(DEVELOPMENT_PROXY);
  });

  it.each(['configureServer', 'configurePreviewServer'] as const)(
    '%s sends negotiated requests to Axum and leaves ordinary HTML in Vite',
    async (hookName) => {
      const upstream = createServer((req, res) => {
        res.writeHead(200, { 'content-type': 'application/vnd.ckbadger.raw+json', vary: 'Accept' });
        res.end(JSON.stringify({ path: req.url, accept: req.headers.accept }));
      });
      await new Promise<void>((resolve) => upstream.listen(0, '127.0.0.1', resolve));
      let middleware: Connect.NextHandleFunction | undefined;
      const plugin = agentFormatProxy(
        `http://127.0.0.1:${(upstream.address() as AddressInfo).port}`
      );
      const hook = plugin[hookName];
      if (typeof hook !== 'function') throw new Error(`Missing ${hookName}`);
      const server = {
        middlewares: {
          use: (fn: Connect.NextHandleFunction) => {
            middleware = fn;
          },
        },
      };
      await hook(server as ViteDevServer & PreviewServer);
      const frontend = createServer((req, res) => {
        if (!middleware) throw new Error('Middleware was not installed');
        middleware(req, res, () => {
          res.end('vite html');
        });
      });
      await new Promise<void>((resolve) => frontend.listen(0, '127.0.0.1', resolve));
      const get = (path: string, accept = 'text/html') =>
        new Promise<string>((resolve, reject) => {
          request(
            {
              hostname: '127.0.0.1',
              port: (frontend.address() as AddressInfo).port,
              path,
              headers: { accept },
            },
            (res) => {
              let body = '';
              res.on('data', (chunk) => {
                body += chunk;
              });
              res.on('end', () => resolve(body));
            }
          )
            .on('error', reject)
            .end();
        });
      try {
        for (const path of ['/testnet/blocks/42.raw', '/testnet/blocks/42?format=raw']) {
          expect(JSON.parse(await get(path)).path).toBe(path);
        }
        expect(JSON.parse(await get('/testnet/blocks/42', 'text/markdown')).accept).toBe(
          'text/markdown'
        );
        expect(await get('/testnet/blocks/42')).toBe('vite html');
        expect(await get('/api/testnet/v1/blocks/42', 'text/markdown')).toBe('vite html');
      } finally {
        await Promise.all(
          [frontend, upstream].map(
            (server) =>
              new Promise<void>((resolve, reject) =>
                server.close((error) => (error ? reject(error) : resolve()))
              )
          )
        );
      }
    }
  );
});
