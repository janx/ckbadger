import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': resolve(__dirname, '.'),
    },
  },
  test: {
    // jsdom with one set of fetch and abort primitives (Node's), as a browser has one realm
    environment: './__tests__/jsdom-environment.ts',
    setupFiles: ['./__tests__/setup.ts'],
    include: ['__tests__/**/*.test.{ts,tsx}'],
    exclude: ['node_modules', '.next'],
    // A whole test: its waits (each capped at 10 s, __tests__/setup.ts) and its own rendering. The
    // longest test under two full suites at once ran 7.8 s, so 15 s is about twice that, and a
    // wait that never resolves fails on its own 10 s cap first, with Testing Library's message.
    testTimeout: 15000,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcov'],
      include: ['components/**', 'hooks/**', 'lib/**'],
      exclude: ['**/*.d.ts', '**/*.test.{ts,tsx}'],
    },
    globals: true,
  },
});
