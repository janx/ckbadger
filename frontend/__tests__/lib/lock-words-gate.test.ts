// Lane B4 second fix round (review-rules-fixes-tests SHOULD-3, the coordinator's item 5): one
// decision writes a lock's words (`lockWords` in components/ui/lock-kind.tsx), off the lock's
// script where the place has it. A read of a lock's name anywhere else is one of three kinds, each
// site listed here by file, kind and count:
//
// - it feeds the decision (the row's facts handed to `rowLock` / `LockKind` / `cellLockFacts`);
// - it is the decision's words already written (a tile's `lockKind`, `lockWordsText`'s output);
// - a known writer outside the decision, pinned by number in a list whose total is pinned (the
//   places the lanes have not reached). A new read fails here.
//
// Lane B4 third round (review-rules-fixes2 SHOULD 3, the coordinator's item 6): the gate reads the
// TypeScript syntax tree, not a pattern over the text. A read is any of
// - the API's name for a lock (`lockKind`, `lockName`) by property access, a string key in
//   brackets, or destructuring;
// - the registry's name in an address's script info (`lockScriptInfo.name`, by any of those forms);
// - the registry's name looked up by a code hash (`lookup?.[codeHash]?.name`, a destructured
//   `{ name }` of one, or `registeredName(lookup, codeHash)` on any code hash but a type's).
// What it cannot place it refuses: a computed key that may name a lock, `lockScriptInfo` taken
// whole or by a computed key, `registeredName` passed as a value or called without its code hash.
// The old gate (a regex for `.lockKind` / `.lockName`) found 21 reads in 12 files, 14 of them in
// six known files; this one finds what the tables below pin.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..', '..');
const API_NAMES = new Set(['lockKind', 'lockName']);

type Kind = 'api name' | 'script info name' | 'registry name';
type Reads = Record<string, Partial<Record<Kind, number>>>;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path);
    return /\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

function strip(e: ts.Expression): ts.Expression {
  while (ts.isParenthesizedExpression(e) || ts.isNonNullExpression(e) || ts.isAsExpression(e))
    e = e.expression;
  return e;
}

/** The name an expression ends in (`a.b?.c` → c, `a['c']` → c, `c` → c), or null. */
function terminalName(e: ts.Expression): string | null {
  e = strip(e);
  if (ts.isIdentifier(e)) return e.text;
  if (ts.isPropertyAccessExpression(e)) return e.name.text;
  if (ts.isElementAccessExpression(e) && ts.isStringLiteralLike(e.argumentExpression))
    return e.argumentExpression.text;
  return null;
}

/** A registry lookup by a code hash: `lookup?.[codeHash]`, `lookup.data?.[g.script.codeHash]`. */
function isLookupByCodeHash(e: ts.Expression, source: ts.SourceFile): boolean {
  e = strip(e);
  if (!ts.isElementAccessExpression(e)) return false;
  const key = e.argumentExpression;
  if (ts.isStringLiteralLike(key) || ts.isNumericLiteral(key)) return false;
  return /codeHash/i.test(key.getText(source)) || /lookup/i.test(e.expression.getText(source));
}

/**
 * A use of `lockScriptInfo` the gate can place: a property of it read (by name or a string key), or
 * it destructured by an object pattern (whose `name` is then counted).
 */
function placesLockScriptInfo(node: ts.Expression): boolean {
  let outer: ts.Node = node;
  while (ts.isNonNullExpression(outer.parent) || ts.isParenthesizedExpression(outer.parent))
    outer = outer.parent;
  const parent = outer.parent;
  if (ts.isPropertyAccessExpression(parent)) return parent.expression === outer;
  if (ts.isElementAccessExpression(parent))
    return parent.expression === outer && ts.isStringLiteralLike(parent.argumentExpression);
  return (
    ts.isVariableDeclaration(parent) &&
    parent.initializer === outer &&
    ts.isObjectBindingPattern(parent.name)
  );
}

/** A computed key that may name a lock's name (not its hash, code hash or args). */
const LOCK_KEY = /(\block|Lock)(?!(Script)?(Hash|CodeHash|Args)\b|\.(codeHash|hashType|args)\b)/;

/** A binding element's key: `{ key }`, `{ key: alias }`, `{ 'key': alias }`; computed keys refused. */
function bindingKey(node: ts.BindingElement, refuse: (n: ts.Node, why: string) => void) {
  const prop = node.propertyName ?? node.name;
  if (ts.isComputedPropertyName(prop)) {
    refuse(node, 'a computed key in a destructuring');
    return null;
  }
  if (ts.isIdentifier(prop) || ts.isStringLiteralLike(prop)) return prop.text;
  return null;
}

/** The reads of a lock's name in one source, by kind; what it cannot place it refuses. */
export function lockNameReads(text: string, fileName: string): Partial<Record<Kind, number>> {
  const source = ts.createSourceFile(
    fileName,
    text,
    ts.ScriptTarget.Latest,
    true,
    fileName.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const out: Partial<Record<Kind, number>> = {};
  const count = (kind: Kind) => (out[kind] = (out[kind] ?? 0) + 1);
  const refuse = (node: ts.Node, why: string): never => {
    const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
    throw new Error(`${fileName}:${line + 1}: ${why}: ${node.getText(source)}`);
  };
  const visit = (node: ts.Node) => {
    if (ts.isPropertyAccessExpression(node)) {
      const name = node.name.text;
      if (API_NAMES.has(name)) count('api name');
      else if (name === 'lockScriptInfo') {
        if (!placesLockScriptInfo(node)) refuse(node, 'lockScriptInfo taken whole');
      } else if (name === 'name') {
        if (terminalName(node.expression) === 'lockScriptInfo') count('script info name');
        else if (isLookupByCodeHash(node.expression, source)) count('registry name');
      }
    } else if (ts.isElementAccessExpression(node)) {
      const key = node.argumentExpression;
      if (ts.isStringLiteralLike(key)) {
        if (API_NAMES.has(key.text)) count('api name');
        else if (key.text === 'lockScriptInfo') {
          if (!placesLockScriptInfo(node)) refuse(node, 'lockScriptInfo taken whole');
        } else if (key.text === 'name') {
          if (terminalName(node.expression) === 'lockScriptInfo') count('script info name');
          else if (isLookupByCodeHash(node.expression, source)) count('registry name');
        }
      } else if (LOCK_KEY.test(key.getText(source)))
        refuse(node, 'a computed key that may name a lock');
      else if (terminalName(node.expression) === 'lockScriptInfo')
        refuse(node, 'lockScriptInfo read by a computed key');
    } else if (ts.isBindingElement(node) && ts.isObjectBindingPattern(node.parent)) {
      const key = bindingKey(node, refuse);
      if (key !== null && API_NAMES.has(key)) count('api name');
      else if (key === 'lockScriptInfo') {
        if (!ts.isObjectBindingPattern(node.name)) refuse(node, 'lockScriptInfo bound whole');
      } else if (key === 'name') {
        const holder = node.parent.parent;
        if (ts.isBindingElement(holder) && bindingKey(holder, refuse) === 'lockScriptInfo')
          count('script info name');
        else if (ts.isVariableDeclaration(holder) && holder.initializer) {
          if (terminalName(holder.initializer) === 'lockScriptInfo') count('script info name');
          else if (isLookupByCodeHash(holder.initializer, source)) count('registry name');
        }
      }
    } else if (ts.isIdentifier(node) && node.text === 'registeredName') {
      const parent = node.parent;
      if (ts.isCallExpression(parent) && parent.expression === node) {
        const codeHash = parent.arguments[1];
        if (codeHash === undefined) refuse(parent, 'registeredName without its code hash');
        // a type's code hash names a type, not a lock
        else if (!/\btype\??\.codeHash$/.test(codeHash.getText(source))) count('registry name');
      } else if (
        !(ts.isFunctionDeclaration(parent) && parent.name === node) &&
        !ts.isImportSpecifier(parent) &&
        !ts.isExportSpecifier(parent)
      )
        refuse(node, 'registeredName passed as a value');
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

/** Each file's reads of a lock's name, by kind. */
function reads(): Reads {
  const out: Reads = {};
  for (const dir of ['app', 'components', 'lib'])
    for (const path of files(join(ROOT, dir))) {
      const rel = relative(ROOT, path);
      if (rel === 'components/ui/lock-kind.tsx') continue;
      const found = lockNameReads(readFileSync(path, 'utf8'), rel);
      if (Object.keys(found).length > 0) out[rel] = found;
    }
  return out;
}

/** The tables, summed by file and kind (one file can hold a feed and a known writer). */
function merged(...tables: Reads[]): Reads {
  const out: Reads = {};
  for (const table of tables)
    for (const [file, kinds] of Object.entries(table))
      for (const [kind, n] of Object.entries(kinds) as [Kind, number][]) {
        out[file] ??= {};
        out[file][kind] = (out[file][kind] ?? 0) + n;
      }
  return out;
}
const total = (table: Reads) =>
  Object.values(table).reduce(
    (sum, kinds) => sum + Object.values(kinds).reduce((a, b) => a + (b ?? 0), 0),
    0
  );

/** Reads handed to the decision: a row's facts (`rowLock`), a party's, a cell's (`cellLockFacts`). */
const FEEDS: Reads = {
  'components/asset/asset-history.tsx': { 'api name': 1 },
  'components/asset/collection-holders.tsx': { 'api name': 1 },
  'components/asset/collection-objects.tsx': { 'api name': 1 },
  'components/asset/token-holders.tsx': { 'api name': 1 },
  'components/dao/dao-board.tsx': { 'api name': 1 },
  // the address's activity rows: the address's party, its name handed to the row's decision
  'components/address/address-history.tsx': { 'script info name': 1 },
  // the one rule for a registry name (`registeredName`), and a tile's lock name handed to
  // `cellLockFacts`
  'components/transaction/transaction-cell-tiles.tsx': { 'registry name': 2 },
  // "Lock of output i": the output's name handed to `cellLockFacts`
  'app/tx/[hash]/client-page.tsx': { 'registry name': 1 },
};
/** The decision's words already written: a tile's `lockKind` (`lockWordsText` of the decision). */
const DECIDED: Reads = {
  'components/ui/cell-tile.tsx': { 'api name': 2 },
  // the owner's words `ownerOf` wrote, taken apart for an input's and an output's tile
  'components/transaction/transaction-cell-tiles.tsx': { 'api name': 2 },
};
/**
 * Known writers outside the decision. The second round's six: 1 cell versions, 2 item evidence,
 * 3 m-NFT issue note, 4 object hero prose, 5 identity overview, 6 identity evidence. Found by the
 * syntax tree (third round), the four writers review-rules-fixes2 SHOULD 3 names, left for the
 * chain page lane: 7 the address page's Lock metric, 8 the cell page's Lock fact (its registry
 * lookup and the fact's `lockName`), 9 the cell's spend requirement (its `lockName`s, and the
 * structure's group names, a third copy of the registry rule), 10 the microscope's readout; and
 * three it found besides: 11 the address page's kind line and lock byte map (`lockScriptInfo.name`,
 * their words and their links), 12 the transaction page's script groups (a lock group's name and
 * link) and the cellbase witness's lock.
 */
const KNOWN: Reads = {
  'components/asset/cell-versions.tsx': { 'api name': 1 },
  'components/asset/item-evidence.tsx': { 'api name': 1 },
  'components/asset/mnft-detail.tsx': { 'api name': 1 },
  // 6 → 5 (lane p-assets, R8): the life sentence names the lock it sits under once, its story
  // and its lede alike
  'components/asset/object-hero.tsx': { 'api name': 5 },
  'components/identity/identity-overview.tsx': { 'api name': 4 },
  'components/identity/identity-evidence.tsx': { 'api name': 1 },
  'components/address/address-holdings.tsx': { 'script info name': 1 },
  'app/cell/[outpoint]/client-page.tsx': { 'api name': 1, 'registry name': 1 },
  // registry name 1 → 0 (lane D3): the structure's group names, the third copy of the registry
  // rule (9), went with the list; the cell page's groups are the rows of script-group-rows.tsx
  'components/cell/cell-transactions.tsx': { 'api name': 2 },
  'components/cell/cell-microscope.tsx': { 'api name': 1 },
  // 4 → 2 (lane D2): the kind line's lock link and name went to the status strip, whose lock is
  // address-holdings.tsx's one read
  'app/address/[addr]/client-page.tsx': { 'script info name': 2 },
  // 3 → 1 (lane D3): the cellbase witness's lock stays; the script groups' name and link (12)
  // moved with the rows to script-group-rows.tsx, where one read gives both
  'app/tx/[hash]/client-page.tsx': { 'registry name': 1 },
  // known writer 12 moved, not a new writer (lane D3): the transaction page's script groups, the
  // rows the cell page now draws too, their two reads (a group's name, its link) made one
  'components/transaction/script-group-rows.tsx': { 'registry name': 1 },
};

describe('the gate reads the syntax, every form (lane B4 third round)', () => {
  it('counts destructuring, a string key in brackets and property access of the API’s name', () => {
    expect(
      lockNameReads(
        `const { lockKind } = row; const { lockName: n } = cell; f(row['lockKind'], row?.lockName);`,
        'x.ts'
      )
    ).toEqual({ 'api name': 4 });
  });

  it('counts the script info’s name by property, bracket and destructuring', () => {
    expect(
      lockNameReads(
        `a.lockScriptInfo?.name; a.lockScriptInfo['name']; const { name } = a.lockScriptInfo!;
         const { lockScriptInfo: { name: m } } = a; a.lockScriptInfo?.deprecated;`,
        'x.ts'
      )
    ).toEqual({ 'script info name': 4 });
  });

  it('counts a registry lookup by a code hash, and registeredName on any code hash but a type’s', () => {
    expect(
      lockNameReads(
        `lookup?.[codeHash]?.name; lookup.data?.[g.script.codeHash]?.name;
         const { name } = lookup[cell.lock.codeHash]; registeredName(lookup, out.lock.codeHash);
         registeredName(lookup, g.script.codeHash); registeredName(lookup, cell.type.codeHash);
         rows[0]?.name; list[i].name;`,
        'x.ts'
      )
    ).toEqual({ 'registry name': 5 });
  });

  it('refuses what it cannot place', () => {
    const refused = (text: string) =>
      expect(() => lockNameReads(text, 'x.ts')).toThrow(/x\.ts:1: /);
    refused(`const { [key]: v } = row;`);
    refused(`row[lockKey];`);
    refused(`row[field.rowLock];`);
    expect(
      lockNameReads(`blocks[blocks.length - 1]; byLock[lockHash]; m[cell.lock.codeHash];`, 'x.ts')
    ).toEqual({});
    refused(`const info = a.lockScriptInfo;`);
    refused(`a.lockScriptInfo?.[k];`);
    refused(`const { lockScriptInfo } = a;`);
    refused(`names.map(registeredName);`);
    refused(`registeredName(lookup);`);
  });
});

describe('a lock’s words have one writer (lane B4)', () => {
  it('reads a lock’s name only where it feeds the decision, or a known site', () => {
    expect(reads()).toEqual(merged(FEEDS, DECIDED, KNOWN));
  });

  // the old gate (a regex): 21 reads in 12 files, the known writers six files and 14 reads
  // Re-pinned on purpose (lane D2; renamed, it was "… 41 reads in 20 files, … 28 reads"): the
  // address page's kind line read the lock's name twice for its link; the status strip's lock is
  // address-holdings.tsx's one read, so the known reads fall 28 → 26 and all 41 → 39
  // Re-pinned on purpose (lane D3; renamed, it was "… 39 reads in 20 files, … 12 files and 26
  // reads"): the transaction page's script groups (known writer 12) moved to their own module,
  // components/transaction/script-group-rows.tsx, and read the registry once for a group's name and
  // its link where they read it twice: the known reads fall 26 → 25 and all 39 → 38, in one more
  // file. Then (lane D3, renamed again, it was "… 38 reads in 21 files, … 13 files and 25 reads")
  // the cell page's own copy of the groups' names went: 25 → 24 known reads, 38 → 37 in all
  // Re-pinned on purpose (lane p-assets; renamed, it was "… 37 reads in 21 files, … 13 files and
  // 24 reads"): the object life sentence reads the lock it sits under once (known writer 4), 24 → 23
  // known reads and 37 → 36 in all
  it('pins the totals: 36 reads in 21 files, the known writers 13 files and 23 reads', () => {
    const all = merged(FEEDS, DECIDED, KNOWN);
    expect(Object.keys(all)).toHaveLength(21);
    expect(total(all)).toBe(36);
    expect(Object.keys(KNOWN)).toHaveLength(13);
    expect(total(KNOWN)).toBe(23);
  });
});
