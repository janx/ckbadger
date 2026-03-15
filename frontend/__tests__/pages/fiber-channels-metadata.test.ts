import { describe, expect, it } from 'vitest';
import { metadata } from '@/app/fiber/channels/page';

describe('fiber channels page metadata', () => {
  it('exports the approved Fiber Channels head copy', () => {
    expect(metadata.title).toBe('Fiber Channels');
    expect(metadata.description).toBe(
      'Follow the living circuitry of Fiber on Nervos, where nodes whisper value through channels like signals across a sleepless mind.'
    );
  });
});
