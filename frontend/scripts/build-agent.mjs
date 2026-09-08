import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';
import vm from 'node:vm';

const root = fileURLToPath(new URL('..', import.meta.url));
await build({
  configFile: false,
  root,
  resolve: { alias: { '@': root } },
  build: {
    target: 'es2022',
    outDir: 'server-dist',
    lib: {
      entry: resolve(root, 'lib/ai/server-entry.ts'),
      name: 'CkbadgerAgent',
      formats: ['iife'],
      fileName: () => 'agent-renderer.js',
    },
  },
});

// Discovery is emitted from exactly the registry used by the bundled renderer.
const bundle = await readFile(resolve(root, 'server-dist/agent-renderer.js'), 'utf8');
const context = vm.createContext({ URLSearchParams });
vm.runInContext(bundle, context);
const registry = context.CkbadgerAgent.routeDocumentation();
for (const name of ['llms.txt', 'llms-full.txt']) {
  const source = await readFile(resolve(root, 'public', name), 'utf8');
  const marker = '<!-- REGISTERED_PAGE_FORMATS -->';
  if (!source.includes(marker)) throw new Error(`${name} is missing ${marker}`);
  await writeFile(resolve(root, 'dist', name), source.replace(marker, registry));
}
