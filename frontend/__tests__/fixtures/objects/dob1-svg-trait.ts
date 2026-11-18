// The decode answer of 0x707532f6…c7ba (mainnet), a DOB/1 object whose IMAGE trait holds its
// picture as an SVG, read from /spore/objects/{id}/decode on 2026-11-18; the traits are cut to
// two and the SVG to its frame. Its page did not render (the owner's report): the picture /render
// draws stood in `media` with size 0 and hash '', and the decoder output list refused the empty
// hash ("shorten: an empty hash").
import type { SporeDobDecoded } from '@/lib/api';
export const DOB1_SVG_ID = '0x707532f6bc2be3b13df205315ba96e149d095276d2706e4d9a2dabce635ac7ba';
export const DOB1_SVG_STEP0_HASH =
  'd0111ca8e5ef2f8c740252b8b9b7f72774561a3db8466556f1ccd56b7de0d606';
export const DOB1_SVG_STEP1_HASH =
  '7a8245b77f07febc016d2520e46ee499da9a499f1c10dc7964ac2d56f7af37b0';
/** The decode answer of an object with this id (the page's own id in a page test). */
export function dob1SvgDecoded(sporeId: string = DOB1_SVG_ID): SporeDobDecoded {
  return {
    status: 'decoded',
    sporeId,
    contentType: 'dob/1',
    dnaHex: null,
    traits: [
      { name: 'CellNumber', value: '5' },
      {
        name: 'IMAGE',
        value: "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 300 300'></svg>",
      },
    ],
    media: [
      {
        mediaType: 'application/json',
        role: null,
        size: 456,
        hash: DOB1_SVG_STEP0_HASH,
        step: 0,
        url: `/spore/objects/${sporeId}/media/${DOB1_SVG_STEP0_HASH}`,
      },
      {
        mediaType: 'application/json',
        role: null,
        size: 22953,
        hash: DOB1_SVG_STEP1_HASH,
        step: 1,
        url: `/spore/objects/${sporeId}/media/${DOB1_SVG_STEP1_HASH}`,
      },
      {
        mediaType: 'image/svg+xml',
        role: 'render',
        size: 0,
        hash: '',
        step: null,
        url: `/spore/objects/${sporeId}/render`,
      },
    ],
    issues: [],
  };
}
