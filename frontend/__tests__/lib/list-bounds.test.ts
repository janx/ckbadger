import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { genesisBlock, genesisTransactions } from '@/__tests__/msw/fixtures/genesis-block';
import { WIDEST } from '@/__tests__/utils/shell-budget';
import { resourceScale } from '@/components/blocks/block-visual';
import { trackPx, MIN_TRACK_PX } from '@/components/ui/aligned-bar';
import { formatTick } from '@/lib/chart-ticks';
import { formatAgeOneUnit } from '@/lib/format-age';
import { SHORT_FORMS, shortenAddress, shortenHash } from '@/lib/shorten';
import {
  GENESIS_BLOCK_BYTES,
  MAX_BLOCK_BYTES,
  MAX_BLOCK_CYCLES,
  TX_CYCLES_BOUND,
  WIDEST_AGE,
  WIDEST_BLOCK_FEES,
  WIDEST_BLOCK_NUMBER,
  WIDEST_BLOCK_REWARD,
  WIDEST_CKB_HELD,
  WIDEST_COMPACT_TICK,
  WIDEST_COUNT,
  WIDEST_COUNT_DIGITS,
  WIDEST_DAO_DEPOSIT,
  WIDEST_FEE_RATE,
  WIDEST_POOL_VALUE_LINE,
  WIDEST_PROPOSALS,
  WIDEST_RANK,
  WIDEST_ROW_COUNT,
  WIDEST_TX_FEE,
  WIDEST_TX_OUTPUT,
  WIDEST_TX_VALUE,
  shortChars,
} from '@/lib/list-bounds';

// Readability F3-e: each bound is the widest value a list column's formatter writes, so a column
// budgets the text the page draws, not a number written by hand.
describe('the widest values a list column holds (lib/list-bounds)', () => {
  it('writes each bound as its column’s formatter writes it', () => {
    expect(WIDEST_BLOCK_NUMBER).toBe('#99,999,999');
    expect(WIDEST_COUNT).toBe('9,999');
    expect(WIDEST_PROPOSALS).toBe('1,500');
    expect(WIDEST_COUNT_DIGITS).toBe('9999');
    expect(WIDEST_ROW_COUNT).toBe('99,999,999');
    expect(WIDEST_RANK).toBe('999,999');
    expect(WIDEST_FEE_RATE).toBe('1,000,000,000');
    // the column form, every decimal
    expect(WIDEST_BLOCK_REWARD).toBe('9,999.99999999');
    expect(WIDEST_BLOCK_FEES).toBe('999.99999999');
    expect(WIDEST_TX_VALUE).toBe('999,999,999.99999999');
    expect(WIDEST_TX_FEE).toBe('99.99999999');
    expect(WIDEST_TX_OUTPUT).toBe('99,999,999,999.99999999');
    expect(WIDEST_POOL_VALUE_LINE).toBe('999,999.99999999');
    expect(WIDEST_DAO_DEPOSIT).toBe('9,999,999,999.99999999');
    // the summary form, whole from 1,000 up
    expect(WIDEST_CKB_HELD).toBe('99,999,999,999');
  });

  it('is the age the one age form writes for a block ten years and 364 days old', () => {
    const genesis = Date.UTC(2019, 10, 15, 16, 0, 0); // 2019-11-16 00:00 UTC+8
    expect(formatAgeOneUnit(genesis, Date.UTC(2030, 10, 15, 0, 0, 0))).toBe(WIDEST_AGE);
    // what the oldest page writes today is narrower
    expect(formatAgeOneUnit(genesis, Date.UTC(2026, 10, 14, 0, 0, 0))).toBe('6y 363d ago');
  });

  it('counts an id shortened to a form as lib/shorten writes it', () => {
    const hash = `0x${'0'.repeat(64)}`;
    const address = `ckb1${'q'.repeat(93)}`;
    expect(shortChars(SHORT_FORMS.row.hash)).toBe(shortenHash(hash, 'row').length);
    expect(shortChars(SHORT_FORMS.tight.hash)).toBe(shortenHash(hash, 'tight').length);
    expect(shortChars(SHORT_FORMS.row.address)).toBe(shortenAddress(address, 'row').length);
    expect([17, 11, 13]).toEqual([
      shortChars(SHORT_FORMS.row.hash),
      shortChars(SHORT_FORMS.tight.hash),
      shortChars(SHORT_FORMS.row.address),
    ]);
  });

  it('puts the chain’s limits on the scales the bar heads write', () => {
    expect(resourceScale([GENESIS_BLOCK_BYTES]).toLocaleString('en-US')).toBe('1,500,000');
    expect(resourceScale([MAX_BLOCK_BYTES]).toLocaleString('en-US')).toBe('600,000');
    expect(resourceScale([MAX_BLOCK_CYCLES]).toLocaleString('en-US')).toBe('4,000,000,000');
    expect(resourceScale([TX_CYCLES_BOUND]).toLocaleString('en-US')).toBe('750,000,000');
  });

  it('never writes a tick in the tick form wider than four characters, up to the cycle limit', () => {
    let widest = '';
    for (let v = 1; v <= MAX_BLOCK_CYCLES; v = Math.ceil(v * 1.01) + 1) {
      const scale = resourceScale([v]);
      const tick = formatTick(scale, scale);
      if (tick.length > widest.length) widest = tick;
    }
    expect(widest).toHaveLength(WIDEST_COMPACT_TICK.length);
    expect(WIDEST_COMPACT_TICK).toHaveLength(4);
  });
});

// Reviews fb-tests 6 and fb-logic 6 (the lead's ruling): every bound is one of two kinds and says
// which. A consensus bound states its arithmetic from the protocol's limits, and is tested here
// against that arithmetic written again from the protocol's own numbers (mainnet's get_consensus,
// as the API crate pins it, and the molecule sizes of a transaction's parts), not against its own
// string. A bound of record holds by no protocol limit (a fee, a value, a rank, a height years
// ahead): it states the figure it claims and where that figure comes from.
describe('every bound says its kind (reviews fb-tests 6, fb-logic 6)', () => {
  const ROOT = join(__dirname, '..', '..');
  const source = ts.createSourceFile(
    'list-bounds.ts',
    readFileSync(join(ROOT, 'lib', 'list-bounds.ts'), 'utf8'),
    ts.ScriptTarget.Latest,
    true
  );
  /** Each exported constant of lib/list-bounds.ts and the doc comment above it. */
  const exported = source.statements.flatMap((s) => {
    if (!ts.isVariableStatement(s)) return [];
    if (!s.modifiers?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword)) return [];
    const doc = ts
      .getJSDocCommentsAndTags(s.declarationList.declarations[0])
      .map((d) => (typeof d.comment === 'string' ? d.comment : ''))
      .join(' ');
    return s.declarationList.declarations.map((d) => ({ name: d.name.getText(), doc }));
  });
  const bounds = exported.filter((e) => e.name !== 'UNAVAILABLE');
  const kind = (doc: string) =>
    /^Consensus:/.test(doc) ? 'consensus' : /^Of record:/.test(doc) ? 'of record' : null;

  it('marks each bound consensus or of record, and an of-record bound names its source', () => {
    expect(bounds.filter((b) => kind(b.doc) === null).map((b) => b.name)).toEqual([]);
    for (const b of bounds.filter((b) => kind(b.doc) === 'of record'))
      expect([b.name, /Source: \S/.test(b.doc)]).toEqual([b.name, true]);
    const named = (k: string) =>
      bounds
        .filter((b) => kind(b.doc) === k)
        .map((b) => b.name)
        .sort();
    expect({ consensus: named('consensus'), 'of record': named('of record') }).toEqual({
      consensus: [
        'WIDEST_COUNT',
        'WIDEST_PROPOSALS',
        'WIDEST_COUNT_DIGITS',
        'MAX_TX_INPUTS',
        'MAX_BLOCK_BYTES',
        'MAX_BLOCK_CYCLES',
        'WIDEST_COMPACT_TICK',
      ].sort(),
      'of record': [
        'WIDEST_BLOCK_NUMBER',
        'WIDEST_AGE',
        'GENESIS_BLOCK_BYTES',
        'LARGEST_TX_BYTES',
        'TX_CYCLES_BOUND',
        'WIDEST_BLOCK_REWARD',
        'WIDEST_BLOCK_FEES',
        'WIDEST_TX_VALUE',
        'WIDEST_TX_OUTPUT',
        'WIDEST_TX_FEE',
        'WIDEST_POOL_VALUE_LINE',
        'WIDEST_FEE_RATE',
        'WIDEST_DAO_DEPOSIT',
        'WIDEST_RANK',
        'WIDEST_CKB_HELD',
        'WIDEST_ROW_COUNT',
      ].sort(),
    });
  });

  /** Mainnet's get_consensus, as crates/api pins it (its consensus tests read the same file). */
  const consensus = JSON.parse(
    readFileSync(
      join(ROOT, '..', 'crates', 'api', 'tests', 'fixtures', 'mainnet_get_consensus.json'),
      'utf8'
    )
  ) as Record<string, string>;
  const blockBytes = Number(consensus.max_block_bytes);

  it('takes the block limits from the protocol: 597,000 bytes, 3,500,000,000 cycles, 1,500 proposals', () => {
    expect(MAX_BLOCK_BYTES).toBe(blockBytes);
    expect(MAX_BLOCK_CYCLES).toBe(Number(consensus.max_block_cycles));
    expect(WIDEST_PROPOSALS).toBe(
      Number(consensus.max_block_proposals_limit).toLocaleString('en-US')
    );
    expect([blockBytes, MAX_BLOCK_CYCLES]).toEqual([597_000, 3_500_000_000]);
  });

  // The molecule sizes of a transaction's parts (ckb's blockchain.mol): a CellInput is a struct of
  // since (Uint64, 8) and previous_output (OutPoint: tx_hash Byte32, 32, and index Uint32, 4).
  // A CellOutput is a table (4 + 3 × 4 header) of capacity (8), a lock Script (a table, 4 + 3 × 4,
  // of code_hash 32, hash_type 1 and args, a fixvec of at least 4) and no type (0): 77 at least,
  // and in a transaction 4 more for its offset in outputs, 4 for its empty data and 4 for that
  // data's offset. A transaction in a block is a table (4 + 2 × 4) of a RawTransaction (4 + 6 × 4)
  // holding version (4), three fixvecs (4 each empty, inputs at least one input), two empty
  // dynvecs (4 each) and no witnesses (4), and 4 for its offset in the block.
  const INPUT = 8 + 32 + 4;
  const OUTPUT = 4 + 3 * 4 + 8 + (4 + 3 * 4 + 32 + 1 + 4) + 4 + 4 + 4;
  const TRANSACTION = 4 + 2 * 4 + (4 + 6 * 4) + 4 + 4 + 4 + (4 + INPUT) + 4 + 4 + 4 + 4;

  it('bounds a block’s transactions and a transaction’s outputs under 10,000: four digits', () => {
    expect([OUTPUT, TRANSACTION]).toEqual([89, 116]);
    const outputs = Math.floor(blockBytes / OUTPUT);
    const transactions = Math.floor(blockBytes / TRANSACTION);
    expect([outputs, transactions]).toEqual([6_707, 5_146]);
    // genesis, the one block past the limit, holds two transactions, its cellbase 671 outputs
    expect(genesisTransactions.map((tx) => tx.outputsCount)).toEqual([671, 2]);
    for (const n of [outputs, transactions, 671]) {
      expect(n.toLocaleString('en-US').length).toBeLessThanOrEqual(WIDEST_COUNT.length);
      expect(String(n).length).toBeLessThanOrEqual(WIDEST_COUNT_DIGITS.length);
    }
  });

  // Review fb-tests 6: one quantity, two bounds. The tables budget a height of eight digits
  // (WIDEST_BLOCK_NUMBER), the shell's budget nine (__tests__/utils/shell-budget.ts, the shell
  // lane's file). Pinned as a known fault so the list can only shrink: when the shell takes this
  // bound, this case fails and its entry goes.
  const KNOWN_FAULTS = [
    {
      case: 'the status line budgets a block height of nine digits, the tables of eight',
      shortfall:
        'one glyph: "#999,999,999" against "#99,999,999", 7.2 px at the status line’s 12 px',
      lane: 'shell + gates',
      why: '__tests__/utils/shell-budget.ts (WIDEST.block, line 79) is that lane’s file; it should read WIDEST_BLOCK_NUMBER from lib/list-bounds.ts and drop WIDEST.blockToday, the same eight digits',
    },
  ] as const;
  it('keeps one bound for a block height: the known fault of the shell, pinned', () => {
    expect(KNOWN_FAULTS).toHaveLength(1);
    expect(WIDEST.blockToday).toBe(WIDEST_BLOCK_NUMBER);
    expect(WIDEST.block.length - WIDEST_BLOCK_NUMBER.length).toBe(1);
  });
});

describe('trackPx: a bar’s track holds its head’s tick row', () => {
  it('measures the ticks at 12 px a glyph apart, never under 48 px', () => {
    // "0 1,500,000": 11 glyphs at 7.2 px
    expect(trackPx(['0', '1,500,000'])).toBe(79.2);
    expect(trackPx(['0', '4,000,000,000'])).toBe(108);
    expect(trackPx(['0', '600,000'])).toBe(64.8);
    // "0 50 100%": 9 glyphs
    expect(trackPx(['0', '50', '100%'])).toBe(64.8);
    // "0 750M" is 43.2 px: the least track
    expect(trackPx(['0', '750M'])).toBe(MIN_TRACK_PX);
    expect(MIN_TRACK_PX).toBe(48);
  });

  it('refuses an empty tick row', () => {
    expect(() => trackPx([])).toThrow('an empty tick row');
    expect(() => trackPx(['0', ''])).toThrow('an empty tick row');
  });
});
