import { describe, expect, it } from 'vitest';
import { formatObserverTimeUtc8 } from '@/lib/fork-time';

describe('fork observer clock', () => {
  it('renders the same explicit UTC+8 day used to bin node switch observations', () => {
    expect(formatObserverTimeUtc8('2026-10-20T16:00:00+00:00')).toBe('2026-10-21 00:00:00 UTC+8');
  });
  it('rejects malformed persisted detection times', () => {
    expect(() => formatObserverTimeUtc8('not-a-date')).toThrow('invalid fork detection time');
  });
});
