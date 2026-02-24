import { describe, expect, it } from 'vitest';
import { parseSporeClusterDescription } from '@/lib/spore-cluster-description';

describe('parseSporeClusterDescription', () => {
  it('parses plain text description', () => {
    const parsed = parseSporeClusterDescription('A plain cluster description');

    expect(parsed).toEqual({
      summary: 'A plain cluster description',
      metadataEntries: [],
      rawJson: null,
      isJson: false,
    });
  });

  it('parses JSON object and exposes metadata entries', () => {
    const parsed = parseSporeClusterDescription(
      JSON.stringify({
        description: 'Generative spores with on-chain DNA',
        version: 2,
        category: 'art',
        tags: ['dob', 'spore'],
      })
    );

    expect(parsed).not.toBeNull();
    expect(parsed?.summary).toBe('Generative spores with on-chain DNA');
    expect(parsed?.isJson).toBe(true);
    expect(parsed?.metadataEntries).toEqual([
      {
        key: 'version',
        label: 'Version',
        value: '2',
      },
      {
        key: 'category',
        label: 'Category',
        value: 'art',
      },
      {
        key: 'tags',
        label: 'Tags',
        value: 'dob, spore',
      },
    ]);
    expect(parsed?.rawJson).toContain('"version": 2');
  });

  it('parses JSON array metadata', () => {
    const parsed = parseSporeClusterDescription('["a", "b", "c"]');

    expect(parsed).toEqual({
      summary: 'JSON metadata array (3 items)',
      metadataEntries: [],
      rawJson: '[\n  "a",\n  "b",\n  "c"\n]',
      isJson: true,
    });
  });

  it('extracts DOB metadata hints from nested dob object', () => {
    const parsed = parseSporeClusterDescription(
      JSON.stringify({
        description: 'DOB collection metadata',
        dob: {
          ver: 1,
          pattern: [{}, {}, {}],
          decoders: [{}, {}],
        },
      })
    );

    expect(parsed).not.toBeNull();
    expect(parsed?.metadataEntries).toEqual([
      {
        key: 'dob.ver',
        label: 'DOB Version',
        value: '1',
      },
      {
        key: 'dob.pattern',
        label: 'DOB Pattern Items',
        value: '3',
      },
      {
        key: 'dob.decoders',
        label: 'DOB Decoders',
        value: '2',
      },
    ]);
  });
});
