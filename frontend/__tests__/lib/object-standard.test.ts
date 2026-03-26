import { describe, it, expect } from 'vitest';
import {
  classifyObjectStandard,
  getStandardInfo,
  filterDisplayableTraits,
  buildMediaCompositionView,
} from '@/lib/object-standard';

describe('classifyObjectStandard', () => {
  it('classifies dob/0', () => {
    expect(classifyObjectStandard('dob/0')).toBe('dob/0');
  });

  it('classifies dob/1', () => {
    expect(classifyObjectStandard('dob/1')).toBe('dob/1');
  });

  it('classifies other dob versions as dob/1', () => {
    expect(classifyObjectStandard('dob/2')).toBe('dob/1');
    expect(classifyObjectStandard('dob/99')).toBe('dob/1');
  });

  it('classifies plain images', () => {
    expect(classifyObjectStandard('image/png')).toBe('plain-image');
    expect(classifyObjectStandard('image/jpeg')).toBe('plain-image');
    expect(classifyObjectStandard('image/webp')).toBe('plain-image');
  });

  it('classifies SVG separately from other images', () => {
    expect(classifyObjectStandard('image/svg+xml')).toBe('plain-svg');
  });

  it('classifies plain text', () => {
    expect(classifyObjectStandard('text/plain')).toBe('plain-text');
    expect(classifyObjectStandard('text/html')).toBe('plain-text');
  });

  it('classifies unknown types as generic', () => {
    expect(classifyObjectStandard('application/octet-stream')).toBe('generic');
    expect(classifyObjectStandard('')).toBe('generic');
  });

  it('is case-insensitive', () => {
    expect(classifyObjectStandard('DOB/0')).toBe('dob/0');
    expect(classifyObjectStandard('Image/PNG')).toBe('plain-image');
  });
});

describe('getStandardInfo', () => {
  it('returns DOB decode support for dob types', () => {
    expect(getStandardInfo('dob/0').supportsDobDecode).toBe(true);
    expect(getStandardInfo('dob/1').supportsDobDecode).toBe(true);
  });

  it('returns no DOB decode support for plain types', () => {
    expect(getStandardInfo('image/png').supportsDobDecode).toBe(false);
    expect(getStandardInfo('text/plain').supportsDobDecode).toBe(false);
  });

  it('returns parsing method for known types', () => {
    expect(getStandardInfo('dob/0').parsingMethod).toContain('DOB/0');
    expect(getStandardInfo('dob/1').parsingMethod).toContain('DOB/1');
    expect(getStandardInfo('image/png').parsingMethod).toContain('image');
  });

  it('returns null parsing method for generic', () => {
    expect(getStandardInfo('application/octet-stream').parsingMethod).toBeNull();
  });
});

describe('filterDisplayableTraits', () => {
  const svgTrait = { name: 'SVG', value: '<svg xmlns="..."><rect/></svg>' };
  const longTrait = { name: 'Data', value: 'x'.repeat(600) };
  const dataImageTrait = { name: 'Img', value: 'data:image/png;base64,abc' };
  const normalTrait = { name: 'Background', value: 'Blue' };

  it('DOB/0: keeps all traits including SVG', () => {
    const result = filterDisplayableTraits('dob/0', [svgTrait, normalTrait]);
    expect(result).toHaveLength(2);
  });

  it('DOB/1: filters out SVG blob traits', () => {
    const result = filterDisplayableTraits('dob/1', [svgTrait, normalTrait]);
    expect(result).toEqual([normalTrait]);
  });

  it('DOB/1: keeps long non-media traits', () => {
    const result = filterDisplayableTraits('dob/1', [longTrait, normalTrait]);
    expect(result).toEqual([longTrait, normalTrait]);
  });

  it('DOB/1: filters out data:image traits', () => {
    const result = filterDisplayableTraits('dob/1', [dataImageTrait, normalTrait]);
    expect(result).toEqual([normalTrait]);
  });

  it('plain-image: passes through all traits', () => {
    const result = filterDisplayableTraits('plain-image', [normalTrait]);
    expect(result).toEqual([normalTrait]);
  });

  it('handles empty traits', () => {
    expect(filterDisplayableTraits('dob/1', [])).toEqual([]);
  });
});

describe('buildMediaCompositionView', () => {
  const emptyProfile = { sources: [], issues: [] };

  it('plain spore: no decoded items, shows raw payload', () => {
    const view = buildMediaCompositionView('image/png', emptyProfile, [], 'hello');
    expect(view.standard).toBe('plain-image');
    expect(view.decodedItems).toEqual([]);
    expect(view.rawPayload).toBe('hello');
  });

  it('plain spore: no raw payload when no text', () => {
    const view = buildMediaCompositionView('image/png', emptyProfile, [], null);
    expect(view.rawPayload).toBeNull();
  });

  it('DOB/0: labels decoded output correctly', () => {
    const media = [
      {
        mediaType: 'application/json',
        role: null,
        size: 100,
        hash: 'abc',
        step: 0,
        url: '/media/abc',
      },
    ];
    const view = buildMediaCompositionView('dob/0', emptyProfile, media, null);
    expect(view.standard).toBe('dob/0');
    expect(view.decodedItems).toHaveLength(1);
    expect(view.decodedItems[0].label).toBe('Decoded Output');
    expect(view.rawPayload).toBeNull();
  });

  it('DOB/0: labels render item correctly', () => {
    const media = [
      { mediaType: 'image/svg+xml', role: 'render', size: 0, hash: '', step: null, url: '/render' },
    ];
    const view = buildMediaCompositionView('dob/0', emptyProfile, media, null);
    expect(view.decodedItems[0].label).toBe('SVG Render');
    expect(view.decodedItems[0].description).toContain('DOB/0');
  });

  it('DOB/1: labels decoder chain output correctly', () => {
    const media = [
      {
        mediaType: 'application/json',
        role: null,
        size: 14094,
        hash: 'abc',
        step: 1,
        url: '/media/abc',
      },
      { mediaType: 'image/svg+xml', role: 'render', size: 0, hash: '', step: null, url: '/render' },
    ];
    const view = buildMediaCompositionView('dob/1', emptyProfile, media, null);
    expect(view.standard).toBe('dob/1');
    expect(view.decodedItems).toHaveLength(2);
    expect(view.decodedItems[0].label).toBe('Decoder Chain Output');
    expect(view.decodedItems[0].description).toContain('2-decoder chain');
    expect(view.decodedItems[1].label).toBe('SVG Render');
    expect(view.decodedItems[1].description).toContain('DOB/1');
  });

  it('DOB/1: no raw payload when media exists', () => {
    const media = [
      {
        mediaType: 'application/json',
        role: null,
        size: 100,
        hash: 'abc',
        step: 1,
        url: '/media/abc',
      },
    ];
    const view = buildMediaCompositionView('dob/1', emptyProfile, media, 'some text');
    expect(view.rawPayload).toBeNull();
  });

  it('passes through off-chain sources', () => {
    const profile = {
      sources: [{ uri: 'ipfs://Qm123', scheme: 'ipfs', sourceLocation: 'payload_text' }],
      issues: ['some issue'],
    };
    const view = buildMediaCompositionView('dob/0', profile, [], null);
    expect(view.offChainSources).toEqual(profile.sources);
    expect(view.issues).toEqual(['some issue']);
  });

  it('includes parsing method from standard info', () => {
    const view = buildMediaCompositionView('dob/1', emptyProfile, [], null);
    expect(view.parsingMethod).toContain('DOB/1');
  });
});
