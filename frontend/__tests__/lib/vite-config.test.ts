// @vitest-environment node

import type { ProxyOptions, UserConfig } from 'vite';
import viteConfig, { DEVELOPMENT_PROXY, SHARED_FRONTEND_ORIGIN } from '@/vite.config';

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
  });
});
