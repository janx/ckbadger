/**
 * The forty glyphs of the seals: pictographs in one hand, drawn in a unit box (x right, y down),
 * read from a hash: its first byte picks the glyph (`byte mod 40`), the next bits pick the
 * glyph's own variants. Sixteen elements (sky, land and goods: the oracle bones' own forms but
 * metal, which is after the early bronze script), the twelve animals of the years (walking, a
 * bit turns each to face the other way) and the twelve signs of the ecliptic (their symbols
 * written with the brush).
 *
 * Ported from the design prototype's `kinds5.js` (2026-11-10 rubbing × Song print, rounds 16 and
 * 17, `docs/superpowers/design/2026-11-10-rubbing-song/src/kinds5.js`) number for number; the
 * fidelity test (`__tests__/lib/seal-fidelity.test.ts`) holds every glyph to the prototype's.
 */

import { valueOf, type BitReader, type BitSlice } from '@/lib/seal/bits';
import type { Pt } from '@/lib/seal/brush';
import type { Bits, BrushSpec, FigureStroke } from '@/lib/seal/textures';

/* ---------- the brushes of the glyphs ---------- */

/** The incised stroke: pointed at both ends, a little heavier at a turn. */
export const CUT: BrushSpec = {
  curve: 'corner',
  r: 0.07,
  over: 0,
  entry: 'point',
  entryLen: 0.42,
  press0: 0.14,
  exit: 'taper',
  tail: 0.75,
  pulse: 0,
  bulge: 0.24,
  bulgeSigma: 0.22,
  hmul: 1,
};
/** The same stroke bent. */
export const ARC: BrushSpec = {
  curve: 'spline',
  tension: 5,
  entry: 'point',
  entryLen: 0.42,
  press0: 0.12,
  exit: 'taper',
  tail: 0.75,
  pulse: 0,
  bulge: 0,
  hmul: 1,
};
/** A leg: set down and stopped, no tapering. */
export const LEG: BrushSpec = {
  curve: 'corner',
  r: 0.06,
  over: 0,
  entry: 'round',
  press0: 0.1,
  exit: 'blunt',
  pulse: 0,
  bulge: 0.1,
  hmul: 1,
};
/** A short stroke set down round and lifted. */
export const TICK: BrushSpec = {
  curve: 'corner',
  entry: 'round',
  press0: 0.25,
  exit: 'taper',
  tail: 0.5,
  pulse: 0,
  hmul: 1,
};
/** A dot: set down round, stopped. */
export const DOT: BrushSpec = {
  curve: 'corner',
  entry: 'round',
  press0: 0.3,
  exit: 'blunt',
  hmul: 1,
};

/* ---------- drawing in the unit box ---------- */

const D = Math.PI / 180;

const S = (pts: Pt[], brush: BrushSpec, weight?: number): FigureStroke => ({ pts, brush, weight });

/** n + 1 points of the ellipse (cx, cy, rx, ry) from a0° to a1°, y down. */
export function ell(
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  a0: number,
  a1: number,
  n: number
): Pt[] {
  const pts: Pt[] = [];
  for (let i = 0; i <= n; i++) {
    const a = (a0 + ((a1 - a0) * i) / n) * D;
    pts.push([cx + rx * Math.cos(a), cy + ry * Math.sin(a)]);
  }
  return pts;
}

const dot = (x: number, y: number): FigureStroke =>
  S(
    [
      [x - 0.035, y - 0.012],
      [x + 0.035, y + 0.014],
    ],
    DOT,
    1.25
  );

const mirrorX = (strokes: FigureStroke[]): FigureStroke[] =>
  strokes.map((s) => ({ ...s, pts: s.pts.map(([x, y]): Pt => [1 - x, y]) }));

/** An animal faces right; a set bit turns it to face left. */
const face = (flip: number, strokes: FigureStroke[]): FigureStroke[] =>
  flip ? mirrorX(strokes) : strokes;

/** Turned by `deg` about the box's centre, then moved `sx` to the right. */
const turn = (strokes: FigureStroke[], deg: number, sx?: number): FigureStroke[] => {
  const c = Math.cos(deg * D),
    s = Math.sin(deg * D);
  return strokes.map((st) => ({
    ...st,
    pts: st.pts.map(([x, y]): Pt => {
      const dx = x - 0.5,
        dy = y - 0.5;
      return [0.5 + (dx * c - dy * s) + (sx || 0), 0.5 + dx * s + dy * c];
    }),
  }));
};

/* ---------- the forty ---------- */

export const ELEMENTS = [
  'sun',
  'moon',
  'peaks',
  'water',
  'fire',
  'earth',
  'metal',
  'wood',
  'person',
  'rain',
  'cloud',
  'cowrie',
  'skein',
  'jar',
  'arrow',
  'chariot',
] as const;
export const ANIMALS = [
  'rat',
  'ox',
  'tiger',
  'rabbit',
  'dragon',
  'snake',
  'horse',
  'goat',
  'monkey',
  'rooster',
  'dog',
  'pig',
] as const;
export const SIGNS = [
  'aries',
  'taurus',
  'gemini',
  'cancer',
  'leo',
  'virgo',
  'libra',
  'scorpio',
  'archer',
  'capricorn',
  'aquarius',
  'pisces',
] as const;
/** The forty in the order a hash's first byte reads them (`byte mod 40`). */
export const GLYPHS = [...ELEMENTS, ...ANIMALS, ...SIGNS] as const;
export type GlyphName = (typeof GLYPHS)[number];

/** What each glyph is called, in English and in Chinese. */
export const GLYPH_LABEL: Readonly<Record<GlyphName, readonly [string, string]>> = {
  sun: ['sun', '日'],
  moon: ['moon', '月'],
  peaks: ['mountain', '山'],
  water: ['water', '水'],
  fire: ['fire', '火'],
  earth: ['earth', '土'],
  metal: ['metal', '金'],
  wood: ['wood', '木'],
  person: ['person', '人'],
  rain: ['rain', '雨'],
  cloud: ['cloud', '云'],
  cowrie: ['cowrie', '贝'],
  skein: ['thread', '丝'],
  jar: ['jar', '酉'],
  arrow: ['arrow', '矢'],
  chariot: ['chariot', '车'],
  rat: ['rat', '鼠'],
  ox: ['ox', '牛'],
  tiger: ['tiger', '虎'],
  rabbit: ['rabbit', '兔'],
  dragon: ['dragon', '龙'],
  snake: ['snake', '蛇'],
  horse: ['horse', '马'],
  goat: ['goat', '羊'],
  monkey: ['monkey', '猴'],
  rooster: ['rooster', '鸡'],
  dog: ['dog', '狗'],
  pig: ['pig', '猪'],
  aries: ['Aries', '白羊'],
  taurus: ['Taurus', '金牛'],
  gemini: ['Gemini', '双子'],
  cancer: ['Cancer', '巨蟹'],
  leo: ['Leo', '狮子'],
  virgo: ['Virgo', '处女'],
  libra: ['Libra', '天秤'],
  scorpio: ['Scorpio', '天蝎'],
  archer: ['Sagittarius', '射手'],
  capricorn: ['Capricorn', '摩羯'],
  aquarius: ['Aquarius', '水瓶'],
  pisces: ['Pisces', '双鱼'],
};

/** Drawn from a later script, because the oracle bones have no form for them. */
export const NOT_ORACLE: readonly GlyphName[] = ['metal'];

export interface Glyph {
  /** How many variant bits the glyph reads after the byte that chose it. */
  readonly bits: number;
  make(b: Bits): FigureStroke[];
}

/** A glyph read from a hash: which, the bits that chose it and its variant, and its strokes. */
export interface ChosenGlyph {
  readonly name: GlyphName;
  readonly choice: BitSlice;
  readonly variant: BitSlice;
  readonly strokes: FigureStroke[];
}

/** The glyph at the reader's place: `take(8) mod 40`, then the glyph's own variant bits. */
export function glyphOf(rd: BitReader): ChosenGlyph {
  const choice = rd.read(8);
  const name = GLYPHS[valueOf(choice) % GLYPHS.length];
  const variant = rd.read(GLYPH[name].bits);
  return { name, choice, variant, strokes: GLYPH[name].make(variant.bits) };
}

/* ---- the pictographs, in a unit box (x right, y down); the prototype's table as it stands ---- */
export const GLYPH: Readonly<Record<GlyphName, Glyph>> = {
  /* 日 the sun: a ring and its mark */
  sun: {
    bits: 2,
    make: (b) => {
      const cy = 0.5,
        r = 0.42;
      const out = [
        b[0]
          ? S(ell(0.5, cy, r, r, -105, 268, 22), ARC)
          : S(
              [
                [0.5 - r * 0.5, cy - r],
                [0.5 + r * 0.62, cy - r * 0.94],
                [0.5 + r, cy - r * 0.25],
                [0.5 + r * 0.9, cy + r * 0.62],
                [0.5 + r * 0.25, cy + r],
                [0.5 - r * 0.62, cy + r * 0.88],
                [0.5 - r, cy + r * 0.2],
                [0.5 - r * 0.86, cy - r * 0.6],
                [0.5 - r * 0.42, cy - r * 1.02],
              ],
              CUT
            ),
      ];
      out.push(
        b[1]
          ? dot(0.5, cy)
          : S(
              [
                [0.39, cy],
                [0.61, cy + 0.01],
              ],
              CUT
            )
      );
      return out;
    },
  },
  /* 月 the moon: a crescent, leaning */
  moon: {
    bits: 2,
    make: (b) => {
      const out = [
        S(ell(0.62, 0.5, 0.46, 0.46, 104, 256, 18), ARC),
        S(ell(1.56, 0.5, 1.142, 1.142, 203, 157, 10), ARC),
      ];
      if (b[1])
        out.push(
          S(
            [
              [0.3, 0.41],
              [0.3, 0.6],
            ],
            TICK
          )
        );
      const t = turn(out, -22, 0.13);
      return b[0] ? mirrorX(t) : t;
    },
  },
  /* 山 the mountain: three peaks on a base */
  peaks: {
    bits: 2,
    make: (b) => {
      const side = b[0] ? 0.3 : 0.46,
        mid = b[1] ? 0.04 : 0.16;
      return [
        S(
          [
            [0.03, 0.9],
            [0.2, side],
            [0.35, 0.72],
            [0.5, mid],
            [0.65, 0.72],
            [0.8, side],
            [0.97, 0.9],
          ],
          CUT
        ),
        S(
          [
            [0.03, 0.9],
            [0.97, 0.9],
          ],
          CUT
        ),
      ];
    },
  },
  /* 水 water: the stream and its drops; or three streams (川) */
  water: {
    bits: 3,
    make: (b) => {
      const out = [
        S(
          [
            [0.5, 0.02],
            [0.4, 0.26],
            [0.6, 0.5],
            [0.4, 0.74],
            [0.5, 0.98],
          ],
          ARC
        ),
      ];
      if (b[2]) {
        out.push(
          S(
            [
              [0.2, 0.1],
              [0.12, 0.32],
              [0.27, 0.54],
              [0.12, 0.76],
              [0.2, 0.94],
            ],
            ARC
          )
        );
        out.push(
          S(
            [
              [0.8, 0.06],
              [0.72, 0.28],
              [0.88, 0.5],
              [0.73, 0.72],
              [0.8, 0.9],
            ],
            ARC
          )
        );
      } else
        for (const y of b[1] ? [0.16, 0.44, 0.72] : [0.26, 0.62]) {
          out.push(
            S(
              [
                [0.22, y],
                [0.16, y + 0.17],
              ],
              TICK
            )
          );
          out.push(
            S(
              [
                [0.8, y + 0.05],
                [0.86, y + 0.22],
              ],
              TICK
            )
          );
        }
      return b[0] ? mirrorX(out) : out;
    },
  },
  /* 雨 rain: the sky and what falls from it */
  rain: {
    bits: 3,
    make: (b) => {
      const out = [
        b[0]
          ? S(
              [
                [0.05, 0.3],
                [0.09, 0.12],
                [0.91, 0.12],
                [0.95, 0.3],
              ],
              CUT
            )
          : S(
              [
                [0.06, 0.13],
                [0.94, 0.13],
              ],
              CUT
            ),
      ];
      const rows = [
        b[1] ? [0.2, 0.4, 0.6, 0.8] : [0.25, 0.5, 0.75],
        b[1] ? [0.3, 0.5, 0.7] : [0.37, 0.63],
      ];
      if (b[2]) rows.push(rows[0]);
      rows.forEach((xs, i) => {
        const y = 0.34 + i * (b[2] ? 0.22 : 0.3);
        for (const x of xs)
          out.push(
            S(
              [
                [x, y],
                [x - 0.01, y + (b[2] ? 0.15 : 0.2)],
              ],
              TICK
            )
          );
      });
      return out;
    },
  },
  /* 云 the cloud: two strokes of sky and a curl */
  cloud: {
    bits: 2,
    make: (b) => {
      const out = [
        S(
          [
            [0.1, 0.36],
            [0.9, 0.36],
          ],
          CUT
        ),
        S(
          [
            [0.56, 0.36],
            [0.63, 0.56],
            [0.52, 0.84],
            [0.27, 0.82],
            [0.19, 0.62],
            [0.33, 0.5],
            [0.47, 0.6],
          ],
          ARC
        ),
      ];
      if (b[1])
        out.push(
          S(
            [
              [0.3, 0.13],
              [0.7, 0.13],
            ],
            CUT
          )
        );
      return b[0] ? mirrorX(out) : out;
    },
  },
  /* 矢 the arrow: head, shaft, feathers */
  arrow: {
    bits: 2,
    make: (b) => {
      const out = [
        S(
          [
            [0.5, 0.03],
            [0.5, 0.97],
          ],
          CUT
        ),
        S(
          [
            [0.29, 0.27],
            [0.5, 0.04],
          ],
          CUT
        ),
        S(
          [
            [0.71, 0.27],
            [0.5, 0.04],
          ],
          CUT
        ),
        S(
          [
            [0.31, 0.98],
            [0.5, 0.8],
          ],
          CUT
        ),
        S(
          [
            [0.69, 0.98],
            [0.5, 0.8],
          ],
          CUT
        ),
      ];
      if (b[0])
        out.push(
          S(
            [
              [0.31, 0.85],
              [0.5, 0.67],
            ],
            CUT
          ),
          S(
            [
              [0.69, 0.85],
              [0.5, 0.67],
            ],
            CUT
          )
        );
      if (b[1])
        out.push(
          S(
            [
              [0.37, 0.47],
              [0.63, 0.47],
            ],
            CUT
          )
        );
      return out;
    },
  },
  /* 车 the chariot, from above: two wheels, the axle, the pole and its yoke */
  chariot: {
    bits: 2,
    make: (b) => {
      const out = [
        S(
          [
            [0.03, 0.66],
            [0.97, 0.66],
          ],
          CUT
        ),
        S(ell(0.2, 0.66, 0.15, 0.17, -100, 265, 12), ARC),
        S(ell(0.8, 0.66, 0.15, 0.17, -100, 265, 12), ARC),
        S(
          [
            [0.5, 0.56],
            [0.5, 0.1],
          ],
          CUT
        ),
        S(
          [
            [0.3, 0.12],
            [0.7, 0.12],
          ],
          CUT
        ),
        S(
          [
            [0.41, 0.56],
            [0.59, 0.56],
            [0.59, 0.77],
            [0.41, 0.77],
            [0.41, 0.58],
          ],
          CUT,
          0.85
        ),
      ];
      if (b[0])
        out.push(
          S(
            [
              [0.3, 0.12],
              [0.29, 0.26],
            ],
            TICK
          ),
          S(
            [
              [0.7, 0.12],
              [0.71, 0.26],
            ],
            TICK
          )
        );
      if (b[1])
        out.push(
          S(
            [
              [0.2, 0.5],
              [0.2, 0.82],
            ],
            CUT,
            0.7
          ),
          S(
            [
              [0.8, 0.5],
              [0.8, 0.82],
            ],
            CUT,
            0.7
          )
        );
      return out;
    },
  },
  /* 贝 the cowrie: the shell that was money — the slit and its teeth */
  cowrie: {
    bits: 2,
    make: (b) => {
      const out = [
        S(
          [
            [0.5, 0.03],
            [0.73, 0.2],
            [0.8, 0.5],
            [0.73, 0.8],
            [0.5, 0.97],
            [0.27, 0.8],
            [0.2, 0.5],
            [0.27, 0.2],
            [0.5, 0.03],
            [0.6, 0.08],
          ],
          ARC
        ),
      ];
      out.push(
        b[1]
          ? S(
              [
                [0.53, 0.17],
                [0.46, 0.4],
                [0.54, 0.6],
                [0.47, 0.83],
              ],
              ARC
            )
          : S(
              [
                [0.5, 0.17],
                [0.5, 0.83],
              ],
              CUT
            )
      );
      for (const y of b[0] ? [0.32, 0.5, 0.68] : [0.4, 0.62])
        out.push(
          S(
            [
              [0.38, y],
              [0.62, y + 0.01],
            ],
            CUT,
            0.75
          )
        );
      return out;
    },
  },
  /* 丝 the thread: a twisted skein and its ends — a chain */
  skein: {
    bits: 2,
    make: (b) => {
      const n = b[0] ? 3 : 2,
        y0 = b[1] ? 0.2 : 0.06,
        y1 = 0.78,
        A = 0.2;
      const out: FigureStroke[] = [];
      for (const sgn of [1, -1]) {
        const P: Pt[] = [];
        for (let i = 0; i <= 8 * n; i++) {
          const t = i / (8 * n);
          P.push([0.5 + sgn * A * Math.sin(Math.PI * n * t), y0 + t * (y1 - y0)]);
        }
        out.push(S(P, ARC));
      }
      for (const x of [0.28, 0.5, 0.72])
        out.push(
          S(
            [
              [0.5, y1],
              [x, 0.98],
            ],
            TICK
          )
        );
      if (b[1])
        for (const x of [0.3, 0.5, 0.7])
          out.push(
            S(
              [
                [0.5, y0],
                [x, 0.02],
              ],
              TICK
            )
          );
      return out;
    },
  },
  /* 酉 the jar: a vessel, banded */
  jar: {
    bits: 2,
    make: (b) => {
      const foot = b[1] ? 0.97 : 0.9;
      const out = [
        S(
          [
            [0.28, 0.06],
            [0.72, 0.06],
          ],
          CUT
        ),
        S(
          [
            [0.36, 0.07],
            [0.36, 0.2],
            [0.16, 0.36],
            [0.2, 0.64],
            [b[1] ? 0.5 : 0.36, foot],
          ],
          ARC
        ),
        S(
          [
            [0.64, 0.07],
            [0.64, 0.2],
            [0.84, 0.36],
            [0.8, 0.64],
            [b[1] ? 0.5 : 0.64, foot],
          ],
          ARC
        ),
        S(
          [
            [0.2, 0.44],
            [0.8, 0.44],
          ],
          CUT
        ),
      ];
      if (!b[1])
        out.push(
          S(
            [
              [0.3, 0.93],
              [0.7, 0.93],
            ],
            CUT
          )
        );
      if (b[0])
        out.push(
          S(
            [
              [0.25, 0.62],
              [0.75, 0.62],
            ],
            CUT
          )
        );
      return out;
    },
  },
  /* 木 wood: trunk, two branches, two roots */
  wood: {
    bits: 1,
    make: (b) => {
      const wide = b[0] ? 0.08 : 0.17;
      return [
        S(
          [
            [0.5, 0.02],
            [0.5, 0.98],
          ],
          CUT
        ),
        S(
          [
            [wide, 0.12],
            [0.5, 0.44],
          ],
          CUT
        ),
        S(
          [
            [1 - wide, 0.12],
            [0.5, 0.44],
          ],
          CUT
        ),
        S(
          [
            [0.5, 0.62],
            [wide, 0.95],
          ],
          CUT
        ),
        S(
          [
            [0.5, 0.62],
            [1 - wide, 0.95],
          ],
          CUT
        ),
      ];
    },
  },
  /* 火 fire: three tongues over a round hearth */
  fire: {
    bits: 2,
    make: (b) => {
      const mid = b[1] ? 0.03 : 0.14,
        deep = b[0] ? 0.97 : 0.86;
      return [
        S(
          [
            [0.05, 0.56],
            [0.2, 0.2],
            [0.36, 0.6],
            [0.5, mid],
            [0.64, 0.6],
            [0.8, 0.2],
            [0.95, 0.56],
          ],
          CUT
        ),
        S(
          [
            [0.05, 0.56],
            [0.2, deep - 0.12],
            [0.5, deep],
            [0.8, deep - 0.12],
            [0.95, 0.56],
          ],
          ARC
        ),
      ];
    },
  },
  /* 土 earth: a mound standing on the ground */
  earth: {
    bits: 1,
    make: (b) => [
      S(
        [
          [0.08, 0.92],
          [0.92, 0.92],
        ],
        CUT
      ),
      b[0]
        ? S(
            [
              [0.5, 0.92],
              [0.3, 0.5],
              [0.5, 0.08],
              [0.7, 0.5],
              [0.5, 0.92],
            ],
            CUT
          )
        : S(ell(0.5, 0.5, 0.2, 0.42, 90, 452, 20), ARC),
    ],
  },
  /* 金 metal: no oracle-bone form is known; after the early bronze script — the ingots, and the
     axe under its roof */
  metal: {
    bits: 2,
    make: (b) => {
      const out = [
        S(
          [
            [0.36, 0.36],
            [0.64, 0.05],
          ],
          CUT
        ),
        S(
          [
            [0.92, 0.36],
            [0.64, 0.05],
          ],
          CUT
        ),
        S(
          [
            [0.64, 0.3],
            [0.64, 0.93],
          ],
          CUT
        ),
        S(
          [
            [0.44, 0.62],
            [0.84, 0.62],
          ],
          CUT
        ),
        S(
          [
            [0.36, 0.93],
            [0.94, 0.93],
          ],
          CUT
        ),
        dot(0.15, 0.46),
        dot(0.15, 0.72),
      ];
      if (b[0])
        out.push(
          S(
            [
              [0.48, 0.44],
              [0.8, 0.44],
            ],
            CUT
          )
        );
      if (b[1]) out.push(dot(0.15, 0.2));
      return out;
    },
  },
  /* 人 a person, from the side: the back and the arm */
  person: {
    bits: 1,
    make: (b) => {
      const out = [
        S(
          [
            [0.64, 0.04],
            [0.52, 0.3],
            [0.56, 0.62],
            [0.7, 0.97],
          ],
          ARC
        ),
        S(
          [
            [0.54, 0.24],
            [0.33, 0.42],
            [0.2, 0.64],
          ],
          ARC
        ),
      ];
      return b[0] ? mirrorX(out) : out;
    },
  },
  /* ---- the twelve animals of the years, as pictographs in the same hand. The bones stand
     their beasts on their tails; at fifteen pixels a standing tiger and a standing dog are one
     squiggle, so these walk. b[0] turns each to face the other way. ---- */
  rat: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.3, 0.52],
            [0.08, 0.68],
            [0.3, 0.74],
          ],
          CUT
        ),
        S(
          [
            [0.3, 0.52],
            [0.5, 0.42],
            [0.72, 0.54],
            [0.72, 0.68],
            [0.5, 0.76],
            [0.3, 0.74],
          ],
          ARC
        ),
        S(ell(0.32, 0.4, 0.09, 0.09, -100, 265, 10), ARC, 0.85),
        S(
          [
            [0.42, 0.75],
            [0.4, 0.89],
          ],
          LEG
        ),
        S(
          [
            [0.62, 0.73],
            [0.64, 0.89],
          ],
          LEG
        ),
        S(
          [
            [0.72, 0.6],
            [0.86, 0.7],
            [0.96, 0.52],
            [0.88, 0.28],
          ],
          ARC,
          0.8
        ),
      ]),
  },
  ox: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.3, 0.38],
            [0.48, 0.3],
            [0.82, 0.4],
          ],
          ARC
        ),
        S(
          [
            [0.3, 0.38],
            [0.17, 0.46],
            [0.1, 0.66],
          ],
          ARC
        ),
        S(
          [
            [0.03, 0.2],
            [0.1, 0.38],
            [0.24, 0.36],
            [0.31, 0.14],
          ],
          ARC
        ),
        S(
          [
            [0.34, 0.6],
            [0.58, 0.66],
            [0.78, 0.58],
          ],
          ARC,
          0.85
        ),
        S(
          [
            [0.33, 0.42],
            [0.31, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.44, 0.6],
            [0.46, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.72, 0.6],
            [0.7, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.81, 0.42],
            [0.85, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.82, 0.4],
            [0.93, 0.56],
            [0.91, 0.76],
          ],
          ARC,
          0.8
        ),
      ]),
  },
  tiger: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.28, 0.28],
            [0.12, 0.24],
            [0.04, 0.38],
            [0.17, 0.42],
            [0.05, 0.53],
            [0.28, 0.54],
          ],
          CUT
        ),
        S(
          [
            [0.28, 0.28],
            [0.54, 0.3],
            [0.8, 0.38],
          ],
          ARC
        ),
        S(
          [
            [0.28, 0.54],
            [0.54, 0.6],
            [0.78, 0.56],
          ],
          ARC
        ),
        S(
          [
            [0.22, 0.25],
            [0.25, 0.12],
          ],
          TICK
        ),
        S(
          [
            [0.4, 0.3],
            [0.38, 0.5],
          ],
          TICK
        ),
        S(
          [
            [0.52, 0.3],
            [0.5, 0.52],
          ],
          TICK
        ),
        S(
          [
            [0.64, 0.33],
            [0.62, 0.52],
          ],
          TICK
        ),
        S(
          [
            [0.31, 0.56],
            [0.27, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.4, 0.58],
            [0.44, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.7, 0.58],
            [0.67, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.8, 0.4],
            [0.85, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.8, 0.38],
            [0.94, 0.5],
            [0.97, 0.28],
            [0.86, 0.18],
          ],
          ARC,
          0.85
        ),
      ]),
  },
  rabbit: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.27, 0.56],
            [0.4, 0.4],
            [0.62, 0.36],
            [0.8, 0.5],
            [0.83, 0.74],
          ],
          ARC
        ),
        S(
          [
            [0.27, 0.56],
            [0.12, 0.52],
            [0.09, 0.64],
            [0.3, 0.74],
          ],
          ARC
        ),
        S(
          [
            [0.3, 0.75],
            [0.83, 0.75],
          ],
          CUT,
          0.85
        ),
        S(
          [
            [0.2, 0.5],
            [0.15, 0.22],
            [0.23, 0.05],
            [0.3, 0.22],
            [0.28, 0.5],
          ],
          ARC
        ),
        S(
          [
            [0.31, 0.47],
            [0.4, 0.2],
            [0.5, 0.1],
            [0.48, 0.3],
            [0.38, 0.45],
          ],
          ARC
        ),
        S(ell(0.89, 0.6, 0.065, 0.065, -100, 265, 10), ARC, 0.85),
        S(
          [
            [0.36, 0.76],
            [0.28, 0.9],
          ],
          LEG
        ),
        S(
          [
            [0.62, 0.77],
            [0.86, 0.88],
          ],
          LEG
        ),
      ]),
  },
  dragon: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.34, 0.16],
            [0.56, 0.24],
            [0.63, 0.44],
            [0.42, 0.56],
            [0.34, 0.74],
            [0.54, 0.9],
            [0.86, 0.84],
          ],
          ARC
        ),
        S(
          [
            [0.34, 0.16],
            [0.1, 0.1],
          ],
          CUT
        ),
        S(
          [
            [0.34, 0.2],
            [0.1, 0.3],
          ],
          CUT
        ),
        S(
          [
            [0.38, 0.14],
            [0.52, 0.02],
          ],
          TICK
        ),
        S(
          [
            [0.46, 0.18],
            [0.66, 0.08],
          ],
          TICK
        ),
        S(
          [
            [0.61, 0.42],
            [0.8, 0.36],
          ],
          TICK
        ),
        S(
          [
            [0.36, 0.68],
            [0.18, 0.74],
          ],
          TICK
        ),
        S(
          [
            [0.86, 0.84],
            [0.96, 0.7],
          ],
          TICK
        ),
      ]),
  },
  snake: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.5, 0.03],
            [0.35, 0.15],
            [0.5, 0.26],
            [0.65, 0.15],
            [0.5, 0.03],
          ],
          CUT
        ),
        S(
          [
            [0.5, 0.26],
            [0.3, 0.42],
            [0.68, 0.6],
            [0.34, 0.78],
            [0.62, 0.97],
          ],
          ARC
        ),
      ]),
  },
  horse: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.36, 0.4],
            [0.58, 0.36],
            [0.8, 0.42],
          ],
          ARC
        ),
        S(
          [
            [0.36, 0.4],
            [0.24, 0.14],
            [0.08, 0.4],
          ],
          CUT
        ),
        S(
          [
            [0.24, 0.14],
            [0.3, 0.03],
          ],
          TICK
        ),
        S(
          [
            [0.3, 0.2],
            [0.41, 0.13],
          ],
          TICK
        ),
        S(
          [
            [0.33, 0.28],
            [0.45, 0.21],
          ],
          TICK
        ),
        S(
          [
            [0.36, 0.36],
            [0.48, 0.29],
          ],
          TICK
        ),
        S(
          [
            [0.38, 0.42],
            [0.3, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.45, 0.42],
            [0.5, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.74, 0.44],
            [0.68, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.8, 0.44],
            [0.86, 0.93],
          ],
          LEG
        ),
        S(
          [
            [0.8, 0.42],
            [0.94, 0.52],
            [0.92, 0.8],
          ],
          ARC,
          0.85
        ),
        S(
          [
            [0.84, 0.47],
            [0.99, 0.7],
          ],
          TICK
        ),
      ]),
  },
  goat: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.34, 0.46],
            [0.56, 0.42],
            [0.78, 0.46],
          ],
          ARC
        ),
        S(
          [
            [0.34, 0.46],
            [0.22, 0.3],
            [0.07, 0.4],
          ],
          CUT
        ),
        S(
          [
            [0.22, 0.29],
            [0.3, 0.1],
            [0.45, 0.13],
            [0.43, 0.3],
          ],
          ARC
        ),
        S(
          [
            [0.11, 0.42],
            [0.11, 0.6],
          ],
          TICK
        ),
        S(
          [
            [0.36, 0.48],
            [0.32, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.44, 0.48],
            [0.47, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.7, 0.48],
            [0.67, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.78, 0.47],
            [0.82, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.78, 0.46],
            [0.89, 0.33],
          ],
          TICK
        ),
      ]),
  },
  monkey: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(ell(0.36, 0.17, 0.12, 0.12, -100, 265, 12), ARC),
        S(
          [
            [0.44, 0.26],
            [0.64, 0.4],
            [0.62, 0.66],
            [0.46, 0.8],
          ],
          ARC
        ),
        S(
          [
            [0.48, 0.42],
            [0.27, 0.48],
            [0.18, 0.35],
          ],
          ARC
        ),
        S(
          [
            [0.46, 0.8],
            [0.26, 0.74],
            [0.22, 0.93],
          ],
          CUT
        ),
        S(
          [
            [0.56, 0.77],
            [0.8, 0.9],
            [0.95, 0.7],
            [0.85, 0.5],
            [0.75, 0.6],
          ],
          ARC,
          0.85
        ),
      ]),
  },
  rooster: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.3, 0.42],
            [0.5, 0.34],
            [0.7, 0.46],
            [0.6, 0.66],
            [0.38, 0.64],
            [0.3, 0.42],
          ],
          ARC
        ),
        S(
          [
            [0.33, 0.42],
            [0.24, 0.22],
          ],
          CUT
        ),
        S(ell(0.22, 0.16, 0.07, 0.07, -100, 265, 10), ARC, 0.85),
        S(
          [
            [0.16, 0.1],
            [0.13, 0.01],
          ],
          TICK
        ),
        S(
          [
            [0.22, 0.08],
            [0.22, 0.0],
          ],
          TICK
        ),
        S(
          [
            [0.28, 0.1],
            [0.32, 0.02],
          ],
          TICK
        ),
        S(
          [
            [0.15, 0.18],
            [0.03, 0.22],
          ],
          TICK
        ),
        S(
          [
            [0.68, 0.46],
            [0.84, 0.2],
            [0.97, 0.24],
          ],
          ARC
        ),
        S(
          [
            [0.69, 0.5],
            [0.9, 0.38],
            [0.99, 0.5],
          ],
          ARC
        ),
        S(
          [
            [0.44, 0.65],
            [0.42, 0.9],
            [0.31, 0.95],
          ],
          LEG
        ),
        S(
          [
            [0.54, 0.66],
            [0.57, 0.9],
            [0.68, 0.95],
          ],
          LEG
        ),
      ]),
  },
  dog: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(
          [
            [0.32, 0.44],
            [0.56, 0.42],
            [0.78, 0.44],
          ],
          ARC
        ),
        S(
          [
            [0.32, 0.44],
            [0.22, 0.3],
            [0.05, 0.37],
          ],
          CUT
        ),
        S(
          [
            [0.21, 0.3],
            [0.19, 0.13],
          ],
          TICK
        ),
        S(
          [
            [0.27, 0.31],
            [0.31, 0.15],
          ],
          TICK
        ),
        S(
          [
            [0.34, 0.46],
            [0.29, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.41, 0.46],
            [0.45, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.71, 0.46],
            [0.67, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.78, 0.45],
            [0.83, 0.92],
          ],
          LEG
        ),
        S(
          [
            [0.78, 0.44],
            [0.91, 0.3],
            [0.85, 0.13],
            [0.72, 0.2],
          ],
          ARC,
          0.85
        ),
      ]),
  },
  pig: {
    bits: 1,
    make: (b) =>
      face(b[0], [
        S(ell(0.54, 0.52, 0.3, 0.2, -170, 190, 20), ARC),
        S(
          [
            [0.26, 0.44],
            [0.08, 0.5],
            [0.08, 0.62],
            [0.26, 0.62],
          ],
          CUT
        ),
        S(
          [
            [0.3, 0.36],
            [0.23, 0.24],
            [0.36, 0.32],
          ],
          CUT,
          0.85
        ),
        S(
          [
            [0.38, 0.7],
            [0.36, 0.9],
          ],
          LEG
        ),
        S(
          [
            [0.48, 0.72],
            [0.48, 0.9],
          ],
          LEG
        ),
        S(
          [
            [0.64, 0.72],
            [0.64, 0.9],
          ],
          LEG
        ),
        S(
          [
            [0.74, 0.68],
            [0.76, 0.9],
          ],
          LEG
        ),
        S(
          [
            [0.84, 0.48],
            [0.94, 0.42],
            [0.96, 0.54],
            [0.88, 0.55],
          ],
          ARC,
          0.8
        ),
      ]),
  },
  /* ---- the twelve signs of the ecliptic, their own symbols written with the brush ---- */
  aries: {
    bits: 0,
    make: () => [
      S(
        [
          [0.12, 0.46],
          [0.1, 0.26],
          [0.24, 0.1],
          [0.42, 0.2],
          [0.5, 0.5],
          [0.5, 0.96],
        ],
        ARC
      ),
      S(
        [
          [0.88, 0.46],
          [0.9, 0.26],
          [0.76, 0.1],
          [0.58, 0.2],
          [0.5, 0.5],
        ],
        ARC
      ),
    ],
  },
  taurus: {
    bits: 0,
    make: () => [
      S(ell(0.5, 0.66, 0.25, 0.25, -100, 266, 20), ARC),
      S(
        [
          [0.14, 0.05],
          [0.24, 0.3],
          [0.5, 0.4],
          [0.76, 0.3],
          [0.86, 0.05],
        ],
        ARC
      ),
    ],
  },
  gemini: {
    bits: 0,
    make: () => [
      S(
        [
          [0.1, 0.05],
          [0.5, 0.15],
          [0.9, 0.05],
        ],
        ARC
      ),
      S(
        [
          [0.1, 0.95],
          [0.5, 0.85],
          [0.9, 0.95],
        ],
        ARC
      ),
      S(
        [
          [0.34, 0.13],
          [0.34, 0.87],
        ],
        CUT
      ),
      S(
        [
          [0.66, 0.13],
          [0.66, 0.87],
        ],
        CUT
      ),
    ],
  },
  cancer: {
    bits: 0,
    make: () => {
      const up: Pt[] = [
        [0.92, 0.3],
        [0.6, 0.14],
        [0.28, 0.17],
        [0.13, 0.32],
        [0.27, 0.46],
        [0.41, 0.33],
        [0.3, 0.2],
      ];
      return [
        S(up, ARC),
        S(
          up.map(([x, y]): Pt => [1 - x, 1 - y]),
          ARC
        ),
      ];
    },
  },
  leo: {
    bits: 0,
    make: () => [
      S(ell(0.27, 0.6, 0.13, 0.13, -60, 300, 14), ARC),
      S(
        [
          [0.33, 0.48],
          [0.3, 0.26],
          [0.5, 0.08],
          [0.72, 0.24],
          [0.68, 0.56],
          [0.66, 0.8],
          [0.8, 0.93],
          [0.94, 0.82],
        ],
        ARC
      ),
    ],
  },
  virgo: {
    bits: 0,
    make: () => [
      S(
        [
          [0.06, 0.2],
          [0.14, 0.12],
          [0.18, 0.26],
          [0.18, 0.8],
        ],
        ARC
      ),
      S(
        [
          [0.18, 0.3],
          [0.3, 0.12],
          [0.4, 0.28],
          [0.4, 0.8],
        ],
        ARC
      ),
      S(
        [
          [0.4, 0.3],
          [0.52, 0.12],
          [0.62, 0.28],
          [0.62, 0.72],
          [0.74, 0.92],
        ],
        ARC
      ),
      S(
        [
          [0.62, 0.4],
          [0.8, 0.3],
          [0.92, 0.46],
          [0.8, 0.7],
          [0.52, 0.95],
        ],
        ARC
      ),
    ],
  },
  libra: {
    bits: 0,
    make: () => [
      S(
        [
          [0.08, 0.88],
          [0.92, 0.88],
        ],
        CUT
      ),
      S(
        [
          [0.08, 0.64],
          [0.38, 0.64],
          [0.3, 0.5],
          [0.3, 0.32],
          [0.5, 0.15],
          [0.7, 0.32],
          [0.7, 0.5],
          [0.62, 0.64],
          [0.92, 0.64],
        ],
        ARC
      ),
    ],
  },
  scorpio: {
    bits: 0,
    make: () => [
      S(
        [
          [0.04, 0.2],
          [0.12, 0.12],
          [0.16, 0.26],
          [0.16, 0.8],
        ],
        ARC
      ),
      S(
        [
          [0.16, 0.3],
          [0.28, 0.12],
          [0.38, 0.28],
          [0.38, 0.8],
        ],
        ARC
      ),
      S(
        [
          [0.38, 0.3],
          [0.5, 0.12],
          [0.6, 0.28],
          [0.6, 0.74],
          [0.7, 0.9],
          [0.86, 0.86],
          [0.92, 0.64],
        ],
        ARC
      ),
      S(
        [
          [0.8, 0.74],
          [0.92, 0.62],
        ],
        TICK
      ),
      S(
        [
          [0.92, 0.62],
          [0.99, 0.78],
        ],
        TICK
      ),
    ],
  },
  archer: {
    bits: 0,
    make: () => [
      S(
        [
          [0.1, 0.92],
          [0.88, 0.12],
        ],
        CUT
      ),
      S(
        [
          [0.54, 0.12],
          [0.88, 0.12],
        ],
        CUT
      ),
      S(
        [
          [0.88, 0.12],
          [0.88, 0.46],
        ],
        CUT
      ),
      S(
        [
          [0.22, 0.48],
          [0.54, 0.8],
        ],
        CUT
      ),
    ],
  },
  capricorn: {
    bits: 0,
    make: () => [
      S(
        [
          [0.05, 0.16],
          [0.18, 0.13],
          [0.32, 0.68],
        ],
        CUT
      ),
      S(
        [
          [0.32, 0.68],
          [0.4, 0.3],
          [0.52, 0.14],
          [0.63, 0.3],
          [0.63, 0.62],
          [0.58, 0.84],
          [0.44, 0.95],
        ],
        ARC
      ),
      S(ell(0.79, 0.7, 0.14, 0.15, 180, 545, 14), ARC),
    ],
  },
  aquarius: {
    bits: 0,
    make: () => [
      S(
        [
          [0.05, 0.42],
          [0.2, 0.25],
          [0.35, 0.42],
          [0.5, 0.25],
          [0.65, 0.42],
          [0.8, 0.25],
          [0.95, 0.42],
        ],
        CUT
      ),
      S(
        [
          [0.05, 0.74],
          [0.2, 0.57],
          [0.35, 0.74],
          [0.5, 0.57],
          [0.65, 0.74],
          [0.8, 0.57],
          [0.95, 0.74],
        ],
        CUT
      ),
    ],
  },
  pisces: {
    bits: 0,
    make: () => [
      S(
        [
          [0.14, 0.05],
          [0.34, 0.3],
          [0.34, 0.7],
          [0.14, 0.95],
        ],
        ARC
      ),
      S(
        [
          [0.86, 0.05],
          [0.66, 0.3],
          [0.66, 0.7],
          [0.86, 0.95],
        ],
        ARC
      ),
      S(
        [
          [0.2, 0.5],
          [0.8, 0.5],
        ],
        CUT
      ),
    ],
  },
};
