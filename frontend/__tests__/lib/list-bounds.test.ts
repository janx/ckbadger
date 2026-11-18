import { describe, expect, it } from 'vitest';
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
