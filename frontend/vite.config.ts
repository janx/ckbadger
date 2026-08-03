import { defineConfig, type ProxyOptions } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export const SHARED_FRONTEND_ORIGIN = 'http://127.0.0.1:8100';

export const DEVELOPMENT_PROXY = {
  '/runtime-config.js': SHARED_FRONTEND_ORIGIN,
  '/capabilities': SHARED_FRONTEND_ORIGIN,
  '/api': SHARED_FRONTEND_ORIGIN,
  '/ws': {
    target: SHARED_FRONTEND_ORIGIN,
    ws: true,
  },
} satisfies Record<string, string | ProxyOptions>;

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: DEVELOPMENT_PROXY,
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, '.'),
    },
  },
});
