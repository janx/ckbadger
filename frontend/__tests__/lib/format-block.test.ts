import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';
import { formatBlockNumber } from '@/lib/format-block';

describe('formatBlockNumber', () => {
  it('writes every block height as #N with en-US thousands separators', () => {
    expect(formatBlockNumber(0)).toBe('#0');
    expect(formatBlockNumber(17684111)).toBe('#17,684,111');
    expect(formatBlockNumber(BigInt('20311586'))).toBe('#20,311,586');
    expect(formatBlockNumber('11132140')).toBe('#11,132,140');
  });

  it('rejects anything that is not a block height instead of printing it', () => {
    expect(() => formatBlockNumber(-1)).toThrow('Invalid block number -1');
    expect(() => formatBlockNumber(1.5)).toThrow('Invalid block number 1.5');
    expect(() => formatBlockNumber(Number.MAX_SAFE_INTEGER + 1)).toThrow('Invalid block number');
    expect(() => formatBlockNumber('0x10')).toThrow('Invalid block number "0x10"');
    expect(() => formatBlockNumber(BigInt(-3))).toThrow('Invalid block number -3');
  });
});

/**
 * Site-wide guard (DAO-9, SC-C10, FF-L3, SC-S8): a "#" in front of a block height in page
 * source must come from `formatBlockNumber`, never from a raw value or an ad-hoc locale call.
 */
describe('every #N block height goes through formatBlockNumber', () => {
  const root = join(__dirname, '..', '..');
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts)$/.test(name)) files.push(path);
    }
  };
  walk(join(root, 'app'));
  walk(join(root, 'components'));

  const blockish =
    /block|uncle\w*\.number|tip\w*\.number|includedBy\.number|canonicalAtHeight\.number|pays\.number|arc\.(from|to)|\bdbTip|\bchainTip|\bforkPoint|toLocaleString|fmtInt|formatNumber/i;

  // `#8,234` on the (unused) hero stat row is an epoch number, not a block height.
  const notBlockHeights = new Set([
    'components/hero-stat-row.tsx #{BigInt(match[1]).toLocaleString()}',
  ]);

  it('finds no raw or ad-hoc formatted block height after a "#"', () => {
    const offenders: string[] = [];
    for (const file of files) {
      const lines = readFileSync(file, 'utf8').split('\n');
      const text = lines.join('\n');
      for (const match of text.matchAll(/#\$?\{\s*([^{}]*?)\s*\}/g)) {
        const expr = match[1];
        if (expr.startsWith('formatBlockNumber(')) continue;
        if (!blockish.test(expr)) continue;
        const line = text.slice(0, match.index).split('\n').length;
        if (/new Error\(|throw /.test(lines[line - 1])) continue;
        const at = `${relative(root, file)} #{${expr}}`;
        if (!notBlockHeights.has(at)) offenders.push(`${relative(root, file)}:${line} #{${expr}}`);
      }
      for (const match of text.matchAll(/#<AssetCount value=\{([^}]*block[^}]*)\}/gi))
        offenders.push(`${relative(root, file)} #<AssetCount ${match[1]}>`);
    }
    expect(offenders).toEqual([]);
  });
});
