import type { DecodedMediaItem } from '@/lib/api';

// ---------------------------------------------------------------------------
// Standard classification
// ---------------------------------------------------------------------------

export type ObjectStandard =
  | 'plain-image'
  | 'plain-text'
  | 'plain-svg'
  | 'dob/0'
  | 'dob/1'
  | 'generic';

export function classifyObjectStandard(contentType: string): ObjectStandard {
  const ct = contentType.trim().toLowerCase();
  if (ct === 'dob/0') return 'dob/0';
  if (ct === 'dob/1') return 'dob/1';
  // Other dob/ versions fall back to dob/1 behavior (decoder chain)
  if (ct.startsWith('dob/')) return 'dob/1';
  if (ct === 'image/svg+xml') return 'plain-svg';
  if (ct.startsWith('image/')) return 'plain-image';
  if (ct.startsWith('text/')) return 'plain-text';
  return 'generic';
}

// ---------------------------------------------------------------------------
// Per-standard metadata
// ---------------------------------------------------------------------------

export interface StandardInfo {
  standard: ObjectStandard;
  parsingMethod: string | null;
  supportsDobDecode: boolean;
}

const STANDARD_INFO: Record<ObjectStandard, Omit<StandardInfo, 'standard'>> = {
  'dob/0': {
    parsingMethod: 'DOB/0: single decoder, DNA + pattern in cluster description',
    supportsDobDecode: true,
  },
  'dob/1': {
    parsingMethod: 'DOB/1: multi-step decoder chain executed in CKB-VM',
    supportsDobDecode: true,
  },
  'plain-image': {
    parsingMethod: 'Raw on-chain image in Spore cell data',
    supportsDobDecode: false,
  },
  'plain-svg': {
    parsingMethod: 'Raw on-chain SVG in Spore cell data',
    supportsDobDecode: false,
  },
  'plain-text': {
    parsingMethod: 'Raw on-chain text in Spore cell data',
    supportsDobDecode: false,
  },
  generic: {
    parsingMethod: null,
    supportsDobDecode: false,
  },
};

export function getStandardInfo(contentType: string): StandardInfo {
  const standard = classifyObjectStandard(contentType);
  return { standard, ...STANDARD_INFO[standard] };
}

// ---------------------------------------------------------------------------
// Per-standard trait filtering
// ---------------------------------------------------------------------------

export function filterDisplayableTraits<T extends { value: string }>(
  standard: ObjectStandard,
  traits: T[]
): T[] {
  if (!traits.length) return traits;

  switch (standard) {
    case 'dob/0':
      // DOB/0 traits are always displayable — single decoder never produces blobs
      return traits;

    case 'dob/1':
      // DOB/1 decoder chain can emit raw media payloads as trait values.
      // Keep long non-media text visible in Details; only suppress raw media
      // that is already represented by Preview / Media Compositions.
      return traits.filter((t) => {
        const v = t.value.trim();
        if (v.length >= 4 && v.slice(0, 4).toLowerCase() === '<svg') return false;
        if (v.startsWith('data:image/')) return false;
        return true;
      });

    default:
      // Plain spore / generic: no DOB traits expected
      return traits;
  }
}

// ---------------------------------------------------------------------------
// Per-standard media composition view
// ---------------------------------------------------------------------------

export interface MediaViewItem {
  label: string;
  description: string | null;
  mediaType: string;
  size: number;
  hash: string;
  step: number | null;
  url: string;
}

export interface MediaCompositionView {
  standard: ObjectStandard;
  parsingMethod: string | null;
  decodedItems: MediaViewItem[];
  rawPayload: string | null;
  offChainSources: Array<{
    uri: string;
    scheme: string;
    sourceLocation: string;
  }>;
  issues: string[];
}

export function buildMediaCompositionView(
  contentType: string,
  mediaProfile: {
    sources: Array<{ uri: string; scheme: string; sourceLocation: string }>;
    issues: string[];
  },
  dobMedia: DecodedMediaItem[],
  textPayload: string | null
): MediaCompositionView {
  const info = getStandardInfo(contentType);

  return {
    standard: info.standard,
    parsingMethod: info.parsingMethod,
    decodedItems: buildDecodedItems(info.standard, dobMedia),
    rawPayload: dobMedia.length === 0 && !info.supportsDobDecode ? textPayload : null,
    offChainSources: mediaProfile.sources,
    issues: mediaProfile.issues,
  };
}

// ---------------------------------------------------------------------------
// Per-standard decoded item labeling
// ---------------------------------------------------------------------------

function buildDecodedItems(
  standard: ObjectStandard,
  dobMedia: DecodedMediaItem[]
): MediaViewItem[] {
  if (!dobMedia.length) return [];

  switch (standard) {
    case 'dob/0':
      return dobMedia.map((m) => ({
        label: labelDob0Media(m),
        description: describeDob0Media(m),
        mediaType: m.mediaType,
        size: m.size,
        hash: m.hash,
        step: m.step,
        url: m.url,
      }));

    case 'dob/1':
      return dobMedia.map((m) => ({
        label: labelDob1Media(m),
        description: describeDob1Media(m),
        mediaType: m.mediaType,
        size: m.size,
        hash: m.hash,
        step: m.step,
        url: m.url,
      }));

    default:
      // Plain spore should not have DOB media, but handle gracefully
      return dobMedia.map((m) => ({
        label: 'Media',
        description: null,
        mediaType: m.mediaType,
        size: m.size,
        hash: m.hash,
        step: m.step,
        url: m.url,
      }));
  }
}

// -- DOB/0 labels --

function labelDob0Media(m: DecodedMediaItem): string {
  if (m.role === 'render') return 'SVG Render';
  if (m.role) return m.role;
  return 'Decoded Output';
}

function describeDob0Media(m: DecodedMediaItem): string | null {
  if (m.role === 'render') return 'Built from DOB/0 pattern + decoded traits';
  return null;
}

// -- DOB/1 labels --

function labelDob1Media(m: DecodedMediaItem): string {
  if (m.role === 'render') return 'SVG Render';
  if (m.role) return m.role;
  if (m.mediaType.startsWith('image/')) return 'Decoded Image';
  if (m.mediaType.includes('json')) return 'Decoder Chain Output';
  return 'Decoded Output';
}

function describeDob1Media(m: DecodedMediaItem): string | null {
  if (m.role === 'render') {
    return 'Built on-the-fly from DOB/1 SVG patterns + decoded traits';
  }
  if (m.step != null) {
    const decoderCount = m.step + 1;
    return `Final output of ${decoderCount}-decoder chain (step ${m.step})`;
  }
  return null;
}
