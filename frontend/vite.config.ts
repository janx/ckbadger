import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      'next/link': resolve(__dirname, 'src/next-compat/link.tsx'),
      'next/image': resolve(__dirname, 'src/next-compat/image.tsx'),
      'next/navigation': resolve(__dirname, 'src/next-compat/navigation.tsx'),
      'next/dynamic': resolve(__dirname, 'src/next-compat/dynamic.tsx'),
      '@': resolve(__dirname, '.'),
    },
  },
});
