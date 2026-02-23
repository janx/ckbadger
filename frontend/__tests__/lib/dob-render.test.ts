import { describe, expect, it } from 'vitest';
import { decodeDobContent, extractSporePayload } from '@/lib/dob-render';

function encodeMoleculeBytes(value: Uint8Array): Uint8Array {
  const out = new Uint8Array(4 + value.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, value.length, true);
  out.set(value, 4);
  return out;
}

function encodeSporeData(
  contentType: string,
  contentText: string
): {
  dataHex: string;
  contentTypeStart: number;
  contentTypeEnd: number;
  contentStart: number;
  contentEnd: number;
} {
  const contentTypeBytes = new TextEncoder().encode(contentType);
  const contentBytes = new TextEncoder().encode(contentText);

  const ctField = encodeMoleculeBytes(contentTypeBytes);
  const contentField = encodeMoleculeBytes(contentBytes);
  const offsetContentType = 16;
  const offsetContent = offsetContentType + ctField.length;
  const offsetCluster = offsetContent + contentField.length;
  const totalSize = offsetCluster;

  const buffer = new Uint8Array(totalSize);
  const view = new DataView(buffer.buffer);
  view.setUint32(0, totalSize, true);
  view.setUint32(4, offsetContentType, true);
  view.setUint32(8, offsetContent, true);
  view.setUint32(12, offsetCluster, true);
  buffer.set(ctField, offsetContentType);
  buffer.set(contentField, offsetContent);

  const dataHex = `0x${Array.from(buffer)
    .map((item) => item.toString(16).padStart(2, '0'))
    .join('')}`;

  return {
    dataHex,
    contentTypeStart: offsetContentType + 4,
    contentTypeEnd: offsetContentType + 4 + contentTypeBytes.length,
    contentStart: offsetContent + 4,
    contentEnd: offsetContent + 4 + contentBytes.length,
  };
}

describe('dob-render helpers', () => {
  it('extracts spore payload from cell deterministic segments', () => {
    const encoded = encodeSporeData('dob/0', '{ "dna": "0a0100ff" }');
    const payload = extractSporePayload({
      data: encoded.dataHex,
      dataAnalysis: {
        deterministic: {
          kind: 'spore_cell',
          segments: [
            { label: 'content_type', start: encoded.contentTypeStart, end: encoded.contentTypeEnd },
            { label: 'content', start: encoded.contentStart, end: encoded.contentEnd },
          ],
        },
      },
    });

    expect(payload).not.toBeNull();
    expect(payload?.contentType).toBe('dob/0');
    expect(payload?.textContent).toContain('"dna"');
  });

  it('decodes dob/0 dna and traits from cluster metadata', () => {
    const decoded = decodeDobContent({
      sporeContentType: 'dob/0',
      contentText: '{ "dna": "0a01ff00" }',
      clusterDescription: JSON.stringify({
        dob: {
          ver: 0,
          pattern: [
            {
              traitName: 'Background',
              dobType: 'String',
              dnaOffset: 0,
              dnaLength: 1,
              patternType: 'options',
              traitArgs: ['red', 'blue'],
            },
            {
              traitName: 'Level',
              dobType: 'Number',
              dnaOffset: 1,
              dnaLength: 1,
              patternType: 'range',
              traitArgs: [10, 20],
            },
            {
              traitName: 'Seed',
              dobType: 'Number',
              dnaOffset: 2,
              dnaLength: 2,
              patternType: 'rawNumber',
            },
          ],
        },
      }),
    });

    expect(decoded).not.toBeNull();
    expect(decoded?.dnaHex).toBe('0a01ff00');
    expect(decoded?.traits).toEqual([
      { name: 'Background', value: 'red' },
      { name: 'Level', value: '11' },
      { name: 'Seed', value: '255' },
    ]);
  });

  it('builds dob/1 svg from decoded traits', () => {
    const decoded = decodeDobContent({
      sporeContentType: 'dob/0',
      contentText: '{ "dna": "0100" }',
      clusterDescription: JSON.stringify({
        dob: {
          ver: 1,
          decoders: [
            {
              pattern: [
                {
                  traitName: 'BackgroundColor',
                  dobType: 'String',
                  dnaOffset: 0,
                  dnaLength: 1,
                  patternType: 'options',
                  traitArgs: ['red', 'blue'],
                },
                {
                  traitName: 'Shape',
                  dobType: 'String',
                  dnaOffset: 1,
                  dnaLength: 1,
                  patternType: 'options',
                  traitArgs: ['circle', 'square'],
                },
              ],
            },
            {
              pattern: [
                {
                  imageName: 'IMAGE.0',
                  svgFields: 'attributes',
                  traitName: '',
                  patternType: 'raw',
                  traitArgs: "xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'",
                },
                {
                  imageName: 'IMAGE.0',
                  svgFields: 'elements',
                  traitName: 'BackgroundColor',
                  patternType: 'options',
                  traitArgs: [
                    ['red', "<rect width='100' height='100' fill='red' />"],
                    ['blue', "<rect width='100' height='100' fill='blue' />"],
                  ],
                },
                {
                  imageName: 'IMAGE.0',
                  svgFields: 'elements',
                  traitName: 'Shape',
                  patternType: 'options',
                  traitArgs: [
                    ['circle', "<circle cx='50' cy='50' r='30' fill='white' />"],
                    [['*'], "<rect x='25' y='25' width='50' height='50' fill='white' />"],
                  ],
                },
              ],
            },
          ],
        },
      }),
    });

    expect(decoded).not.toBeNull();
    expect(decoded?.svgMarkup).toContain('<svg ');
    expect(decoded?.svgMarkup).toContain("fill='blue'");
    expect(decoded?.svgMarkup).toContain('<circle');
  });

  it('builds dob/1 svg from array-style pattern', () => {
    const decoded = decodeDobContent({
      sporeContentType: 'dob/0',
      contentText: '{ "dna": "01" }',
      clusterDescription: JSON.stringify({
        dob: {
          ver: 1,
          decoders: [
            {
              pattern: [['BackgroundColor', 'String', 0, 1, 'options', ['red', 'blue']]],
            },
            {
              pattern: [
                [
                  'IMAGE.0',
                  'attributes',
                  '',
                  'raw',
                  "xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'",
                ],
                [
                  'IMAGE.0',
                  'elements',
                  'BackgroundColor',
                  'options',
                  [
                    ['red', "<rect width='100' height='100' fill='red' />"],
                    ['blue', "<rect width='100' height='100' fill='blue' />"],
                  ],
                ],
              ],
            },
          ],
        },
      }),
    });

    expect(decoded).not.toBeNull();
    expect(decoded?.traits).toEqual([{ name: 'BackgroundColor', value: 'blue' }]);
    expect(decoded?.svgMarkup).toContain('<svg ');
    expect(decoded?.svgMarkup).toContain("fill='blue'");
  });

  it('returns null for non-dob content type', () => {
    const decoded = decodeDobContent({
      sporeContentType: 'image/png',
      contentText: null,
      clusterDescription: null,
    });
    expect(decoded).toBeNull();
  });
});
