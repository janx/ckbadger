/**
 * Preview detection for on-chain Spore objects.
 *
 * Determines whether cell content can be rendered as a visual preview
 * (image or SVG) and returns the data needed for rendering.
 */
import type { DecodedMediaItem, DecodedRender } from '@/lib/api';

const MAX_IMAGE_BYTES = 2 * 1024 * 1024; // 2 MB
const MAX_SVG_BYTES = 256 * 1024; // 256 KB

export type PreviewKind =
  | { type: 'image'; dataUrl: string }
  | { type: 'svg'; markup: string }
  | { type: 'media-url'; url: string; mediaType: string }
  | null;

const BINARY_IMAGE_TYPES = new Set([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/webp',
  'image/avif',
  'image/bmp',
]);

function baseMimeType(contentType: string): string {
  return contentType.split(';')[0].trim().toLowerCase();
}

function isBinaryImageType(mime: string): boolean {
  return BINARY_IMAGE_TYPES.has(mime);
}

function isSvgType(mime: string): boolean {
  return mime === 'image/svg+xml';
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function extractSvgFromText(text: string): string | null {
  const svgStart = text.indexOf('<svg');
  if (svgStart === -1) return null;
  const svgEnd = text.lastIndexOf('</svg>');
  if (svgEnd === -1) return null;
  return text.slice(svgStart, svgEnd + '</svg>'.length);
}

/**
 * Detect whether on-chain content can be visually previewed.
 *
 * Priority:
 * 1. DOB decode: the decoder's stored image output (highest step = final product), else the
 *    picture `/render` draws from the traits (the API names one only when no stored output is
 *    an image)
 * 2. Binary image (PNG, JPEG, GIF, WebP, AVIF) → data: URL
 * 3. SVG content (image/svg+xml or text containing <svg>)
 */
export function detectPreview(
  contentType: string,
  contentBytes: Uint8Array | undefined,
  dob:
    | {
        media: ReadonlyArray<Pick<DecodedMediaItem, 'mediaType' | 'url' | 'step'>>;
        render: DecodedRender | null;
      }
    | undefined
): PreviewKind {
  if (dob) {
    const images = dob.media.filter((m) => m.mediaType.startsWith('image/'));
    const picture = images.length > 0 ? [...images].sort((a, b) => b.step - a.step)[0] : dob.render;
    if (picture) return { type: 'media-url', url: picture.url, mediaType: picture.mediaType };
  }

  if (!contentBytes || contentBytes.length === 0) {
    return null;
  }

  const mime = baseMimeType(contentType);

  // Binary image formats
  if (isBinaryImageType(mime) && contentBytes.length <= MAX_IMAGE_BYTES) {
    const base64 = bytesToBase64(contentBytes);
    return { type: 'image', dataUrl: `data:${mime};base64,${base64}` };
  }

  // SVG content type
  if (isSvgType(mime) && contentBytes.length <= MAX_SVG_BYTES) {
    const text = new TextDecoder('utf-8', { fatal: false }).decode(contentBytes);
    const svg = extractSvgFromText(text);
    if (svg) {
      return { type: 'svg', markup: svg };
    }
  }

  // Text-like content that may contain embedded SVG
  if (isTextLikeForPreview(mime) && contentBytes.length <= MAX_SVG_BYTES) {
    const text = new TextDecoder('utf-8', { fatal: false }).decode(contentBytes);
    const svg = extractSvgFromText(text);
    if (svg) {
      return { type: 'svg', markup: svg };
    }
  }

  return null;
}

function isTextLikeForPreview(mime: string): boolean {
  return (
    mime.startsWith('text/') ||
    mime.includes('json') ||
    mime.includes('xml') ||
    mime.includes('javascript')
  );
}
