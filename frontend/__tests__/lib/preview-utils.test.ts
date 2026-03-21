import { describe, expect, it } from 'vitest';
import { detectPreview } from '@/lib/preview-utils';

function textToBytes(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

/** Create a minimal PNG header (8-byte magic + minimal IHDR). */
function fakePngBytes(size: number = 64): Uint8Array {
  const bytes = new Uint8Array(size);
  // PNG magic: 137 80 78 71 13 10 26 10
  bytes.set([137, 80, 78, 71, 13, 10, 26, 10], 0);
  return bytes;
}

describe('detectPreview', () => {
  describe('DOB/1 SVG markup', () => {
    it('returns svg preview when dobSvgMarkup is provided', () => {
      const svg = '<svg xmlns="http://www.w3.org/2000/svg"><circle r="10"/></svg>';
      const result = detectPreview('dob/1', undefined, svg);
      expect(result).toEqual({ type: 'svg', markup: svg });
    });

    it('prefers DOB SVG over binary image content', () => {
      const svg = '<svg><rect/></svg>';
      const result = detectPreview('image/png', fakePngBytes(), svg);
      expect(result).toEqual({ type: 'svg', markup: svg });
    });

    it('returns null when dobSvgMarkup is empty string', () => {
      const result = detectPreview('dob/0', undefined, '');
      expect(result).toBeNull();
    });
  });

  describe('binary image formats', () => {
    it('returns image preview for image/png', () => {
      const bytes = fakePngBytes();
      const result = detectPreview('image/png', bytes, null);
      expect(result).not.toBeNull();
      expect(result!.type).toBe('image');
      if (result!.type === 'image') {
        expect(result!.dataUrl).toMatch(/^data:image\/png;base64,/);
      }
    });

    it('returns image preview for image/jpeg', () => {
      const bytes = new Uint8Array([0xff, 0xd8, 0xff, 0xe0]);
      const result = detectPreview('image/jpeg', bytes, null);
      expect(result).not.toBeNull();
      expect(result!.type).toBe('image');
    });

    it('returns image preview for image/gif', () => {
      const bytes = textToBytes('GIF89a');
      const result = detectPreview('image/gif', bytes, null);
      expect(result).not.toBeNull();
      expect(result!.type).toBe('image');
    });

    it('returns image preview for image/webp', () => {
      const bytes = new Uint8Array(16);
      const result = detectPreview('image/webp', bytes, null);
      expect(result).not.toBeNull();
      expect(result!.type).toBe('image');
    });

    it('returns image preview for image/avif', () => {
      const bytes = new Uint8Array(16);
      const result = detectPreview('image/avif', bytes, null);
      expect(result).not.toBeNull();
      expect(result!.type).toBe('image');
    });

    it('handles content type with parameters', () => {
      const bytes = fakePngBytes();
      const result = detectPreview('image/png; charset=utf-8', bytes, null);
      expect(result).not.toBeNull();
      expect(result!.type).toBe('image');
    });

    it('returns null for empty content bytes', () => {
      const result = detectPreview('image/png', new Uint8Array(0), null);
      expect(result).toBeNull();
    });

    it('returns null for undefined content bytes', () => {
      const result = detectPreview('image/png', undefined, null);
      expect(result).toBeNull();
    });
  });

  describe('SVG content', () => {
    it('returns svg preview for image/svg+xml', () => {
      const svg = '<svg xmlns="http://www.w3.org/2000/svg"><circle r="5"/></svg>';
      const bytes = textToBytes(svg);
      const result = detectPreview('image/svg+xml', bytes, null);
      expect(result).toEqual({ type: 'svg', markup: svg });
    });

    it('extracts svg from text content with surrounding text', () => {
      const content = 'prefix <svg><rect/></svg> suffix';
      const bytes = textToBytes(content);
      const result = detectPreview('image/svg+xml', bytes, null);
      expect(result).toEqual({ type: 'svg', markup: '<svg><rect/></svg>' });
    });

    it('returns null for image/svg+xml without valid svg tag', () => {
      const bytes = textToBytes('not svg content');
      const result = detectPreview('image/svg+xml', bytes, null);
      expect(result).toBeNull();
    });
  });

  describe('text-like content with embedded SVG', () => {
    it('detects SVG in text/plain', () => {
      const content = '<svg viewBox="0 0 100 100"><circle r="50"/></svg>';
      const bytes = textToBytes(content);
      const result = detectPreview('text/plain', bytes, null);
      expect(result).toEqual({ type: 'svg', markup: content });
    });

    it('detects SVG in application/json', () => {
      const content = '{"svg": "<svg><rect/></svg>"}';
      // The embedded SVG would need to be in the raw text
      const svgContent = 'prefix <svg><rect/></svg> end';
      const bytes = textToBytes(svgContent);
      const result = detectPreview('application/json', bytes, null);
      expect(result).toEqual({ type: 'svg', markup: '<svg><rect/></svg>' });
    });

    it('returns null for text without SVG', () => {
      const bytes = textToBytes('hello world, just plain text');
      const result = detectPreview('text/plain', bytes, null);
      expect(result).toBeNull();
    });
  });

  describe('non-previewable content', () => {
    it('returns null for application/octet-stream', () => {
      const bytes = new Uint8Array([0x00, 0x01, 0x02]);
      const result = detectPreview('application/octet-stream', bytes, null);
      expect(result).toBeNull();
    });

    it('returns null for audio types', () => {
      const bytes = new Uint8Array(128);
      const result = detectPreview('audio/mpeg', bytes, null);
      expect(result).toBeNull();
    });

    it('returns null for video types', () => {
      const bytes = new Uint8Array(128);
      const result = detectPreview('video/mp4', bytes, null);
      expect(result).toBeNull();
    });

    it('returns null when all inputs are empty', () => {
      const result = detectPreview('', undefined, null);
      expect(result).toBeNull();
    });
  });
});
