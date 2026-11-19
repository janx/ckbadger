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
//
// Lane cd-fixes-controls (review-cd-tests SHOULD-1): the API's names for a lock are three, the
// registry's name for a party's or a cell row's lock (`lockScriptName`, lib/api.ts) besides
// `lockKind` and `lockName`; `hooks/` is read with `app/`, `components/` and `lib/`; a
// `registeredName` imported under another name is read as itself; and a lookup by a code hash
// through a Map (`names.get(codeHash)?.name`, a destructured `{ name }` of one) is a registry name.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..', '..');
const API_NAMES = new Set(['lockKind', 'lockName', 'lockScriptName']);
/** The directories the gate reads: those every source gate reads (lane cd-fixes-controls: hooks/). */
const READ_DIRS = ['app', 'components', 'hooks', 'lib'];

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

/**
 * A registry lookup by a code hash: `lookup?.[codeHash]`, `lookup.data?.[g.script.codeHash]`, and
 * (lane cd-fixes-controls) a Map's `names.get(codeHash)`.
 */
function isLookupByCodeHash(e: ts.Expression, source: ts.SourceFile): boolean {
  e = strip(e);
  if (
    ts.isCallExpression(e) &&
    ts.isPropertyAccessExpression(e.expression) &&
    e.expression.name.text === 'get' &&
    e.arguments.length === 1
  )
    return /codeHash/i.test(e.arguments[0].getText(source));
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
  // registeredName and the names it is imported under (`import { registeredName as rn }`)
  const registered = new Set(['registeredName']);
  const aliases = (node: ts.Node) => {
    if (
      ts.isImportSpecifier(node) &&
      node.propertyName &&
      ts.isIdentifier(node.propertyName) &&
      node.propertyName.text === 'registeredName'
    )
      registered.add(node.name.text);
    ts.forEachChild(node, aliases);
  };
  aliases(source);
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
    } else if (ts.isIdentifier(node) && registered.has(node.text)) {
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
  for (const dir of READ_DIRS)
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
  // "Lock of output i": the output's name handed to `cellLockFacts`; and the cellbase witness's
  // miner lock (lane P1 follow-up; it was known writer 12's second part), its name handed to
  // `cellLockFacts` with the lock
  'app/tx/[hash]/client-page.tsx': { 'registry name': 2 },
  // a group's registry name, handed to `cellLockFacts` with a lock group's script (lane P1
  // follow-up; it was known writer 12): a type group keeps the name, the decision being a lock's
  'components/transaction/script-group-rows.tsx': { 'registry name': 1 },
  // the cell's lock: the registry's name for its code hash handed to `cellLockFacts` with the
  // lock and its address (lane P1 follow-up; it was known writer 8, with the fact's `lockName`)
  'app/cell/[outpoint]/client-page.tsx': { 'registry name': 1 },
  // the address's lock (`addressLockName`, lane P1 follow-up; it was known writers 7 and 11): the
  // registry's name in its script info, handed to `cellLockFacts` with the lock and the address
  'components/address/address-holdings.tsx': { 'script info name': 1 },
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
  // components/asset/cell-versions.tsx left (lane p-assets): a version's lock is cellOwnerLockPart
  // (writer 1)
  // components/asset/mnft-detail.tsx (1) and components/asset/object-hero.tsx (5) left (lane P2's
  // follow-up, item 6): the Issued note, the life sentence and the lock footnote say the lock
  // through the decision's sentence form (cellLockSentence, cellLockWords). Then the rest of the
  // second round's writers left with them: components/asset/item-evidence.tsx (1, the lock
  // script's name), components/identity/identity-overview.tsx (3, the .bit Cell summary's minted
  // and holding locks; its Owner note was cellOwnerLockPart already) and
  // components/identity/identity-evidence.tsx (1, the name cell's "Lock · …" row)
  // registry name 1 → 0 (lane D3): the structure's group names, the third copy of the registry
  // rule (9), went with the list; the cell page's groups are the rows of script-group-rows.tsx
  // 4 → 2 (lane D2): the kind line's lock link and name went to the status strip, whose lock is
  // address-holdings.tsx's one read
  // 3 → 1 (lane D3): the cellbase witness's lock stays; the script groups' name and link (12)
  // moved with the rows to script-group-rows.tsx, where one read gives both
  // known writer 12 moved, not a new writer (lane D3): the transaction page's script groups, the
  // rows the cell page now draws too, their two reads (a group's name, its link) made one
  // 6 → 5 (lane p-assets, R8): the life sentence names the lock it sits under once, its story
  // and its lede alike
  // 4 → 3 (lane p-assets): the Owner fact's note is cellOwnerLockPart (writer 5's first read);
  // the .bit Cell summary's wallet and holder words stay, prose that names a default lock too
  // app/cell/[outpoint]/client-page.tsx, components/cell/cell-transactions.tsx and
  // components/cell/cell-microscope.tsx left (lane P1 follow-up; writers 8, 9 and 10): the cell
  // page decides its lock once (`cellLockFacts`, `lockWords`), and its Lock fact, its spend
  // requirement and the microscope's readout write the decision's name (`lockLabel`); a lock on
  // the default lock's code hash by data is written by its code hash and "by data"
  // components/address/address-holdings.tsx and app/address/[addr]/client-page.tsx left (lane P1
  // follow-up; writers 7 and 11): the address's lock is named once, `addressLockName`, its one
  // read feeding the decision; the strip and the lock byte map write that name (4 → 2 in lane D2:
  // the kind line's lock link and name went to the strip)
  // app/tx/[hash]/client-page.tsx (3 → 1 in lane D3: the script groups' name and link moved to
  // script-group-rows.tsx, where one read gives both) and script-group-rows.tsx left (lane P1
  // follow-up; known writer 12): the cellbase witness's miner lock and a lock group are named by
  // the decision, their reads feed it
  // 13, found when the gate learned `lockScriptName` (lane cd-fixes-controls), for the topic page
  // lane (p-topics): the Fiber graph's party row writes the registry's name of its lock before
  // "lock hash", outside lockWords
  'components/fiber/fiber-network-graph.tsx': { 'api name': 2 },
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

  // Lane cd-fixes-controls (T SHOULD-1): the API's registry name for a party's or a cell row's
  // lock (lib/api.ts `lockScriptName`), an aliased registeredName and a Map lookup by a code hash
  // walked past
  it('counts the API’s lockScriptName by property, bracket and destructuring', () => {
    expect(
      lockNameReads(
        `party.lockScriptName; row['lockScriptName']; const { lockScriptName: n } = cell;`,
        'x.ts'
      )
    ).toEqual({ 'api name': 3 });
  });

  it('counts an aliased registeredName and a Map lookup by a code hash', () => {
    expect(
      lockNameReads(
        `import { registeredName as rn } from '@/lib/script-names';
         rn(lookup, cell.lock.codeHash); rn(lookup, cell.type.codeHash);
         names.get(codeHash)?.name; byHash.get(out.lock.codeHash)!.name;
         const { name } = names.get(g.codeHash)!; m.get(id)?.name;`,
        'x.ts'
      )
    ).toEqual({ 'registry name': 4 });
  });

  it('reads every directory the source gates read, hooks/ among them', () => {
    expect(READ_DIRS).toEqual(['app', 'components', 'hooks', 'lib']);
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
  // Re-pinned on purpose (lane p-assets; renamed, it was "… 36 reads in 21 files, … 13 files and
  // 23 reads"): a version's lock (cell-versions) and an identity's Owner note read the decision,
  // 23 → 21 known reads in 12 files, 36 → 34 in all in 20
  // Re-pinned on purpose (lane cd-fixes-controls, renamed, it was "… 34 reads in 20 files, … 12
  // files and 21 reads"): the gate learned `lockScriptName`, and the Fiber graph's two reads of it
  // (known writer 13, p-topics) came to light: 21 → 23 known reads in 13 files, 34 → 36 in 21
  // Re-pinned on purpose (lane P2's follow-up, item 6; renamed, it was "… 36 reads in 21 files, …
  // 13 files and 23 reads"): the m-NFT Issued note (known writer 3) and the object hero's prose
  // (known writer 4) say the lock through the decision's sentence form, 23 → 17 known reads in 11
  // files, 36 → 30 in all in 19
  // Re-pinned on purpose (lane P2's follow-up, item 6; renamed, it was "… 30 reads in 19 files, …
  // 11 files and 17 reads"): the item evidence (known writer 2), the identity overview (5) and the
  // identity evidence (6) say the lock through the decision too, 17 → 12 known reads in 8 files,
  // 30 → 25 in all in 16; of the second round's six known writers none is left
  // Re-pinned on purpose (lane P1 follow-up; renamed, it was "… 36 reads in 21 files, … 13 files
  // and 23 reads"): the cell page decides its lock once, its registry read feeds the decision and
  // the three `lockName`s are the decision's name; 23 → 18 known reads in 10 files, 36 → 32 in all
  // in 19
  // Re-pinned on purpose (lane P1 follow-up; renamed, it was "… 32 reads in 19 files, … 10 files
  // and 18 reads"): known writer 12, the cellbase witness's lock and the script groups, feeds the
  // decision; its two reads move to the feeds, 18 → 16 known reads in 8 files, all still 32 in 19
  // Re-pinned on purpose (lane P1 follow-up; renamed, it was "… 32 reads in 19 files, … 8 files and
  // 16 reads"): the address's lock is named once by the decision; the page's two reads went, the
  // holdings' one feeds it: 16 → 13 known reads in 6 files, 32 → 30 in all in 18
  // Re-pinned on purpose (the merge of lane P1's follow-up with lane P2's; renamed, it was "… 25
  // reads in 16 files, … 8 files and 12 reads" on the base): this lane's follow-up took the cell
  // page's, the address's and the transaction page's known writers (7 to 12), 10 known reads in 7
  // files, four of its reads feeding the decision now; 12 → 2 known reads in 1 file (the Fiber
  // graph's, writer 13), 25 → 19 in all in 13
  it('pins the totals: 19 reads in 13 files, the known writers 1 file and 2 reads', () => {
    const all = merged(FEEDS, DECIDED, KNOWN);
    expect(Object.keys(all)).toHaveLength(13);
    expect(total(all)).toBe(19);
    expect(Object.keys(KNOWN)).toHaveLength(1);
    expect(total(KNOWN)).toBe(2);
  });
});
