/** Canonical semantic encodings from the approved site review. */
export const CLASS_COLORS = {
  dao: '#f2c55c',
  tokens: '#5aa6f5',
  objects: '#a98cf5',
  identities: '#f08aa8',
  ckb: '#3a4258',
  other: '#2c3348',
} as const;
export type AssetClass = keyof typeof CLASS_COLORS;
export const CELL_COLORS = {
  capacity: '#dee2ec',
  lock: '#7aa7d6',
  type: '#b5c77a',
  data: '#6fc3d9',
} as const;
export const BITCOIN_COLORS = {
  copper: '#dc8a5a',
  light: '#ffe1cc',
  dark: '#a95a32',
  inner: '#1a110d',
} as const;
export const DATA_COLOR = '#2edba3';
export const HUE_RAMP = ['#2edba3', '#24b68a', '#259775', '#287b64', '#2b6054', '#344e48'] as const;
