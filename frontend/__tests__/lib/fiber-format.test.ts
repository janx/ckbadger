import { describe, expect, it } from 'vitest';
import { FIBER_STATE_WORDS, fiberDuration } from '@/lib/fiber-format';

describe('fiber-format', () => {
  it('reads an exact ms string on the site’s one duration form, never "0 d 0 h 3 m" (FF-F5, FF-C6)', () => {
    expect(fiberDuration('201578', 'lifetime')).toBe('3 m 21 s');
    expect(fiberDuration('14820000', 'lifetime')).toBe('4 h 7 m');
    expect(fiberDuration('703140000', 'lifetime')).toBe('8 d 3 h');
    expect(() => fiberDuration('-5', 'lifetime')).toThrow('Fiber lifetime: invalid duration -5');
    expect(() => fiberDuration('9007199254740993', 'lifetime')).toThrow();
  });

  it('names the four states with the board’s words', () => {
    expect(FIBER_STATE_WORDS).toEqual({
      open: 'open',
      cooperativelyClosed: 'closed together',
      forceClosed: 'force-closed · not settled',
      settled: 'force-closed · settled',
    });
  });
});

describe('epochDelayWords', () => {
  it('says a since delay in words: relative epochs with any fraction, or an absolute epoch', async () => {
    const { epochDelayWords } = await import('@/lib/fiber-format');
    expect(epochDelayWords({ relative: true, epochs: 6, index: 0, length: 1 })).toBe('6 epochs');
    expect(epochDelayWords({ relative: true, epochs: 1, index: 0, length: 1 })).toBe('1 epoch');
    expect(epochDelayWords({ relative: true, epochs: 6, index: 1, length: 2 })).toBe(
      '6 1/2 epochs'
    );
    expect(epochDelayWords({ relative: false, epochs: 180, index: 0, length: 1 })).toBe(
      'until epoch 180'
    );
    expect(() => epochDelayWords({ relative: true, epochs: 1, index: 2, length: 2 })).toThrow(
      'invalid epoch delay'
    );
  });
});
