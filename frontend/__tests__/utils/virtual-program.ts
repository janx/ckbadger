import ts from 'typescript';

/**
 * A TypeScript program over in-memory files under `/v` (`@/` is `/v`), for a source gate's own
 * tests: the gate reads the checker of this program as it reads the real one's. No library is
 * loaded (`noLib`), so a file declares what it calls.
 */
export function virtualProgram(files: Record<string, string>): ts.Program {
  const options: ts.CompilerOptions = {
    noLib: true,
    types: [],
    noEmit: true,
    strict: true,
    jsx: ts.JsxEmit.Preserve,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    target: ts.ScriptTarget.ES2020,
    baseUrl: '/v',
    paths: { '@/*': ['./*'] },
  };
  const at = (path: string) => (path.startsWith('/v/') ? path.slice(3) : null);
  const host = ts.createCompilerHost(options);
  host.getCurrentDirectory = () => '/v';
  host.fileExists = (path) => at(path) !== null && at(path)! in files;
  host.readFile = (path) => (at(path) !== null ? files[at(path)!] : undefined);
  host.directoryExists = (path) =>
    path === '/v' || Object.keys(files).some((f) => `/v/${f}`.startsWith(`${path}/`));
  host.getDirectories = () => [];
  host.getSourceFile = (path, language) => {
    const text = at(path) !== null ? files[at(path)!] : undefined;
    return text === undefined ? undefined : ts.createSourceFile(path, text, language, true);
  };
  return ts.createProgram({ rootNames: Object.keys(files).map((f) => `/v/${f}`), options, host });
}
