/* The seals, round 15 (kinds2 + what the stones now say about their content).
   - a transaction's stone is a LONG RECTANGLE: counting rods at the left for the cells it consumed,
     its round core and glyph in the middle, rods at the right for the cells it created.
   - a cell's stone says the three fields of a cell: the glyph is its LOCK's (who holds it), the
     window round the glyph is its TYPE's class (what it is), the pearls are its CAPACITY (how
     many digits of CKB).
   - a script (a lock is an address) has the tall rounded stone, with the same glyph its cells carry.
   Round 14's notes follow.
   The seals, round 14. Same three ids, the answers to round 13's four objections:
     - a transaction has its own stone, not a block's stone cut in two: a SQUARE HOLDING A ROUND
       CORE. Its square is the form of the block it is written in, its round core is the form of
       the cells it makes: the stone that stands between the two.
     - a cell is that round core taken out: the same glyph, the ring re-struck with its index.
     - the seam seal is not a stone's shape but where a seal is pressed: across the seam of the
       two sheets of a transaction's page (consumed | created).
     - the figures: the talisman's hand and order (a brush, stacked compartments, written
       textures, dividers) with an oracle-bone pictograph where the talisman had its core.
   The glyph is read from the hash's first four bits, so it is the same at every size and in
   every cell the transaction made. */
const S2 = require('./seal2.js');
const B = require('./brush.js');

const f1 = (v) => (Math.round(v * 10) / 10).toString();
const f2 = (v) => (Math.round(v * 100) / 100).toString();
const TAU = Math.PI * 2, D = Math.PI / 180;

/* ---------- the pictographs, in a unit box (x right, y down) ---------- */
/* the incised stroke: pointed at both ends, a little heavier at a turn */
const CUT = { curve: 'corner', r: 0.07, over: 0, entry: 'point', entryLen: 0.42, press0: 0.14, exit: 'taper', tail: 0.75, pulse: 0, bulge: 0.24, bulgeSigma: 0.22, hmul: 1 };
/* the same stroke bent */
const ARC = { curve: 'spline', tension: 5, entry: 'point', entryLen: 0.42, press0: 0.12, exit: 'taper', tail: 0.75, pulse: 0, bulge: 0, hmul: 1 };
/* a leg: set down and stopped, no tapering */
const LEG = { curve: 'corner', r: 0.06, over: 0, entry: 'round', press0: 0.1, exit: 'blunt', pulse: 0, bulge: 0.1, hmul: 1 };
const TICK = { curve: 'corner', entry: 'round', press0: 0.25, exit: 'taper', tail: 0.5, pulse: 0, hmul: 1 };
const DOT = { curve: 'corner', entry: 'round', press0: 0.3, exit: 'blunt', hmul: 1 };
const S = (pts, brush, weight) => ({ pts, brush, weight });
const ell = (cx, cy, rx, ry, a0, a1, n) => { const pts = []; for (let i = 0; i <= n; i++) { const a = (a0 + ((a1 - a0) * i) / n) * D; pts.push([cx + rx * Math.cos(a), cy + ry * Math.sin(a)]); } return pts; };
const dot = (x, y) => S([[x - 0.035, y - 0.012], [x + 0.035, y + 0.014]], DOT, 1.25);
const face = (flip, strokes) => (flip ? strokes.map((st) => Object.assign({}, st, { pts: st.pts.map(([x, y]) => [1 - x, y]) })) : strokes);
const mirrorX = (strokes) => strokes.map((s) => Object.assign({}, s, { pts: s.pts.map(([x, y]) => [1 - x, y]) }));
const turn = (strokes, deg, sx) => { const c = Math.cos(deg * D), s = Math.sin(deg * D); return strokes.map((st) => Object.assign({}, st, { pts: st.pts.map(([x, y]) => { const dx = x - 0.5, dy = y - 0.5; return [0.5 + (dx * c - dy * s) + (sx || 0), 0.5 + dx * s + dy * c]; }) })); };

const GLYPH = {
  /* 日 the sun: a ring and its mark */
  sun: { bits: 2, make: (b) => {
    const cy = 0.5, r = 0.42;
    const out = [b[0] ? S(ell(0.5, cy, r, r, -105, 268, 22), ARC) : S([[0.5 - r * 0.5, cy - r], [0.5 + r * 0.62, cy - r * 0.94], [0.5 + r, cy - r * 0.25], [0.5 + r * 0.9, cy + r * 0.62], [0.5 + r * 0.25, cy + r], [0.5 - r * 0.62, cy + r * 0.88], [0.5 - r, cy + r * 0.2], [0.5 - r * 0.86, cy - r * 0.6], [0.5 - r * 0.42, cy - r * 1.02]], CUT)];
    out.push(b[1] ? dot(0.5, cy) : S([[0.39, cy], [0.61, cy + 0.01]], CUT));
    return out;
  } },
  /* 月 the moon: a crescent, leaning */
  moon: { bits: 2, make: (b) => {
    const out = [S(ell(0.62, 0.5, 0.46, 0.46, 104, 256, 18), ARC), S(ell(1.56, 0.5, 1.142, 1.142, 203, 157, 10), ARC)];
    if (b[1]) out.push(S([[0.3, 0.41], [0.3, 0.6]], TICK));
    const t = turn(out, -22, 0.13);
    return b[0] ? mirrorX(t) : t;
  } },
  /* 山 the mountain: three peaks on a base */
  peaks: { bits: 2, make: (b) => {
    const side = b[0] ? 0.3 : 0.46, mid = b[1] ? 0.04 : 0.16;
    return [S([[0.03, 0.9], [0.2, side], [0.35, 0.72], [0.5, mid], [0.65, 0.72], [0.8, side], [0.97, 0.9]], CUT), S([[0.03, 0.9], [0.97, 0.9]], CUT)];
  } },
  /* 水 water: the stream and its drops; or three streams (川) */
  water: { bits: 3, make: (b) => {
    const out = [S([[0.5, 0.02], [0.4, 0.26], [0.6, 0.5], [0.4, 0.74], [0.5, 0.98]], ARC)];
    if (b[2]) { out.push(S([[0.2, 0.1], [0.12, 0.32], [0.27, 0.54], [0.12, 0.76], [0.2, 0.94]], ARC)); out.push(S([[0.8, 0.06], [0.72, 0.28], [0.88, 0.5], [0.73, 0.72], [0.8, 0.9]], ARC)); } else for (const y of b[1] ? [0.16, 0.44, 0.72] : [0.26, 0.62]) { out.push(S([[0.22, y], [0.16, y + 0.17]], TICK)); out.push(S([[0.8, y + 0.05], [0.86, y + 0.22]], TICK)); }
    return b[0] ? mirrorX(out) : out;
  } },
  /* 雨 rain: the sky and what falls from it */
  rain: { bits: 3, make: (b) => {
    const out = [b[0] ? S([[0.05, 0.3], [0.09, 0.12], [0.91, 0.12], [0.95, 0.3]], CUT) : S([[0.06, 0.13], [0.94, 0.13]], CUT)];
    const rows = [b[1] ? [0.2, 0.4, 0.6, 0.8] : [0.25, 0.5, 0.75], b[1] ? [0.3, 0.5, 0.7] : [0.37, 0.63]];
    if (b[2]) rows.push(rows[0]);
    rows.forEach((xs, i) => { const y = 0.34 + i * (b[2] ? 0.22 : 0.3); for (const x of xs) out.push(S([[x, y], [x - 0.01, y + (b[2] ? 0.15 : 0.2)]], TICK)); });
    return out;
  } },
  /* 云 the cloud: two strokes of sky and a curl */
  cloud: { bits: 2, make: (b) => {
    const out = [S([[0.1, 0.36], [0.9, 0.36]], CUT), S([[0.56, 0.36], [0.63, 0.56], [0.52, 0.84], [0.27, 0.82], [0.19, 0.62], [0.33, 0.5], [0.47, 0.6]], ARC)];
    if (b[1]) out.push(S([[0.3, 0.13], [0.7, 0.13]], CUT));
    return b[0] ? mirrorX(out) : out;
  } },
  /* 矢 the arrow: head, shaft, feathers */
  arrow: { bits: 2, make: (b) => {
    const out = [S([[0.5, 0.03], [0.5, 0.97]], CUT), S([[0.29, 0.27], [0.5, 0.04]], CUT), S([[0.71, 0.27], [0.5, 0.04]], CUT), S([[0.31, 0.98], [0.5, 0.8]], CUT), S([[0.69, 0.98], [0.5, 0.8]], CUT)];
    if (b[0]) out.push(S([[0.31, 0.85], [0.5, 0.67]], CUT), S([[0.69, 0.85], [0.5, 0.67]], CUT));
    if (b[1]) out.push(S([[0.37, 0.47], [0.63, 0.47]], CUT));
    return out;
  } },
  /* 车 the chariot, from above: two wheels, the axle, the pole and its yoke */
  chariot: { bits: 2, make: (b) => {
    const out = [S([[0.03, 0.66], [0.97, 0.66]], CUT), S(ell(0.2, 0.66, 0.15, 0.17, -100, 265, 12), ARC), S(ell(0.8, 0.66, 0.15, 0.17, -100, 265, 12), ARC), S([[0.5, 0.56], [0.5, 0.1]], CUT), S([[0.3, 0.12], [0.7, 0.12]], CUT), S([[0.41, 0.56], [0.59, 0.56], [0.59, 0.77], [0.41, 0.77], [0.41, 0.58]], CUT, 0.85)];
    if (b[0]) out.push(S([[0.3, 0.12], [0.29, 0.26]], TICK), S([[0.7, 0.12], [0.71, 0.26]], TICK));
    if (b[1]) out.push(S([[0.2, 0.5], [0.2, 0.82]], CUT, 0.7), S([[0.8, 0.5], [0.8, 0.82]], CUT, 0.7));
    return out;
  } },
  /* 贝 the cowrie: the shell that was money — the slit and its teeth */
  cowrie: { bits: 2, make: (b) => {
    const out = [S([[0.5, 0.03], [0.73, 0.2], [0.8, 0.5], [0.73, 0.8], [0.5, 0.97], [0.27, 0.8], [0.2, 0.5], [0.27, 0.2], [0.5, 0.03], [0.6, 0.08]], ARC)];
    out.push(b[1] ? S([[0.53, 0.17], [0.46, 0.4], [0.54, 0.6], [0.47, 0.83]], ARC) : S([[0.5, 0.17], [0.5, 0.83]], CUT));
    for (const y of b[0] ? [0.32, 0.5, 0.68] : [0.4, 0.62]) out.push(S([[0.38, y], [0.62, y + 0.01]], CUT, 0.75));
    return out;
  } },
  /* 丝 the thread: a twisted skein and its ends — a chain */
  skein: { bits: 2, make: (b) => {
    const n = b[0] ? 3 : 2, y0 = b[1] ? 0.2 : 0.06, y1 = 0.78, A = 0.2, out = [];
    for (const sgn of [1, -1]) { const P = []; for (let i = 0; i <= 8 * n; i++) { const t = i / (8 * n); P.push([0.5 + sgn * A * Math.sin(Math.PI * n * t), y0 + t * (y1 - y0)]); } out.push(S(P, ARC)); }
    for (const x of [0.28, 0.5, 0.72]) out.push(S([[0.5, y1], [x, 0.98]], TICK));
    if (b[1]) for (const x of [0.3, 0.5, 0.7]) out.push(S([[0.5, y0], [x, 0.02]], TICK));
    return out;
  } },
  /* 酉 the jar: a vessel, banded */
  jar: { bits: 2, make: (b) => {
    const foot = b[1] ? 0.97 : 0.9;
    const out = [S([[0.28, 0.06], [0.72, 0.06]], CUT), S([[0.36, 0.07], [0.36, 0.2], [0.16, 0.36], [0.2, 0.64], [b[1] ? 0.5 : 0.36, foot]], ARC), S([[0.64, 0.07], [0.64, 0.2], [0.84, 0.36], [0.8, 0.64], [b[1] ? 0.5 : 0.64, foot]], ARC), S([[0.2, 0.44], [0.8, 0.44]], CUT)];
    if (!b[1]) out.push(S([[0.3, 0.93], [0.7, 0.93]], CUT));
    if (b[0]) out.push(S([[0.25, 0.62], [0.75, 0.62]], CUT));
    return out;
  } },
  /* 木 wood: trunk, two branches, two roots */
  wood: { bits: 1, make: (b) => {
    const wide = b[0] ? 0.08 : 0.17;
    return [S([[0.5, 0.02], [0.5, 0.98]], CUT), S([[wide, 0.12], [0.5, 0.44]], CUT), S([[1 - wide, 0.12], [0.5, 0.44]], CUT), S([[0.5, 0.62], [wide, 0.95]], CUT), S([[0.5, 0.62], [1 - wide, 0.95]], CUT)];
  } },
  /* 火 fire: three tongues over a round hearth */
  fire: { bits: 2, make: (b) => {
    const mid = b[1] ? 0.03 : 0.14, deep = b[0] ? 0.97 : 0.86;
    return [S([[0.05, 0.56], [0.2, 0.2], [0.36, 0.6], [0.5, mid], [0.64, 0.6], [0.8, 0.2], [0.95, 0.56]], CUT), S([[0.05, 0.56], [0.2, deep - 0.12], [0.5, deep], [0.8, deep - 0.12], [0.95, 0.56]], ARC)];
  } },
  /* 土 earth: a mound standing on the ground */
  earth: { bits: 1, make: (b) => [S([[0.08, 0.92], [0.92, 0.92]], CUT), b[0] ? S([[0.5, 0.92], [0.3, 0.5], [0.5, 0.08], [0.7, 0.5], [0.5, 0.92]], CUT) : S(ell(0.5, 0.5, 0.2, 0.42, 90, 452, 20), ARC)] },
  /* 金 metal: no oracle-bone form is known; after the early bronze script — the ingots, and the
     axe under its roof */
  metal: { bits: 2, make: (b) => {
    const out = [S([[0.36, 0.36], [0.64, 0.05]], CUT), S([[0.92, 0.36], [0.64, 0.05]], CUT), S([[0.64, 0.3], [0.64, 0.93]], CUT), S([[0.44, 0.62], [0.84, 0.62]], CUT), S([[0.36, 0.93], [0.94, 0.93]], CUT), dot(0.15, 0.46), dot(0.15, 0.72)];
    if (b[0]) out.push(S([[0.48, 0.44], [0.8, 0.44]], CUT));
    if (b[1]) out.push(dot(0.15, 0.2));
    return out;
  } },
  /* 人 a person, from the side: the back and the arm */
  person: { bits: 1, make: (b) => {
    const out = [S([[0.64, 0.04], [0.52, 0.3], [0.56, 0.62], [0.7, 0.97]], ARC), S([[0.54, 0.24], [0.33, 0.42], [0.2, 0.64]], ARC)];
    return b[0] ? mirrorX(out) : out;
  } },
  /* ---- the twelve animals of the years, as pictographs in the same hand. The bones stand
     their beasts on their tails; at fifteen pixels a standing tiger and a standing dog are one
     squiggle, so these walk. b[0] turns each to face the other way. ---- */
  rat: { bits: 1, make: (b) => face(b[0], [S([[0.3, 0.52], [0.08, 0.68], [0.3, 0.74]], CUT), S([[0.3, 0.52], [0.5, 0.42], [0.72, 0.54], [0.72, 0.68], [0.5, 0.76], [0.3, 0.74]], ARC), S(ell(0.32, 0.4, 0.09, 0.09, -100, 265, 10), ARC, 0.85), S([[0.42, 0.75], [0.4, 0.89]], LEG), S([[0.62, 0.73], [0.64, 0.89]], LEG), S([[0.72, 0.6], [0.86, 0.7], [0.96, 0.52], [0.88, 0.28]], ARC, 0.8)]) },
  ox: { bits: 1, make: (b) => face(b[0], [S([[0.3, 0.38], [0.48, 0.3], [0.82, 0.4]], ARC), S([[0.3, 0.38], [0.17, 0.46], [0.1, 0.66]], ARC), S([[0.03, 0.2], [0.1, 0.38], [0.24, 0.36], [0.31, 0.14]], ARC), S([[0.34, 0.6], [0.58, 0.66], [0.78, 0.58]], ARC, 0.85), S([[0.33, 0.42], [0.31, 0.93]], LEG), S([[0.44, 0.6], [0.46, 0.93]], LEG), S([[0.72, 0.6], [0.7, 0.93]], LEG), S([[0.81, 0.42], [0.85, 0.93]], LEG), S([[0.82, 0.4], [0.93, 0.56], [0.91, 0.76]], ARC, 0.8)]) },
  tiger: { bits: 1, make: (b) => face(b[0], [S([[0.28, 0.28], [0.12, 0.24], [0.04, 0.38], [0.17, 0.42], [0.05, 0.53], [0.28, 0.54]], CUT), S([[0.28, 0.28], [0.54, 0.3], [0.8, 0.38]], ARC), S([[0.28, 0.54], [0.54, 0.6], [0.78, 0.56]], ARC), S([[0.22, 0.25], [0.25, 0.12]], TICK), S([[0.4, 0.3], [0.38, 0.5]], TICK), S([[0.52, 0.3], [0.5, 0.52]], TICK), S([[0.64, 0.33], [0.62, 0.52]], TICK), S([[0.31, 0.56], [0.27, 0.93]], LEG), S([[0.4, 0.58], [0.44, 0.93]], LEG), S([[0.7, 0.58], [0.67, 0.93]], LEG), S([[0.8, 0.4], [0.85, 0.93]], LEG), S([[0.8, 0.38], [0.94, 0.5], [0.97, 0.28], [0.86, 0.18]], ARC, 0.85)]) },
  rabbit: { bits: 1, make: (b) => face(b[0], [S([[0.27, 0.56], [0.4, 0.4], [0.62, 0.36], [0.8, 0.5], [0.83, 0.74]], ARC), S([[0.27, 0.56], [0.12, 0.52], [0.09, 0.64], [0.3, 0.74]], ARC), S([[0.3, 0.75], [0.83, 0.75]], CUT, 0.85), S([[0.2, 0.5], [0.15, 0.22], [0.23, 0.05], [0.3, 0.22], [0.28, 0.5]], ARC), S([[0.31, 0.47], [0.4, 0.2], [0.5, 0.1], [0.48, 0.3], [0.38, 0.45]], ARC), S(ell(0.89, 0.6, 0.065, 0.065, -100, 265, 10), ARC, 0.85), S([[0.36, 0.76], [0.28, 0.9]], LEG), S([[0.62, 0.77], [0.86, 0.88]], LEG)]) },
  dragon: { bits: 1, make: (b) => face(b[0], [S([[0.34, 0.16], [0.56, 0.24], [0.63, 0.44], [0.42, 0.56], [0.34, 0.74], [0.54, 0.9], [0.86, 0.84]], ARC), S([[0.34, 0.16], [0.1, 0.1]], CUT), S([[0.34, 0.2], [0.1, 0.3]], CUT), S([[0.38, 0.14], [0.52, 0.02]], TICK), S([[0.46, 0.18], [0.66, 0.08]], TICK), S([[0.61, 0.42], [0.8, 0.36]], TICK), S([[0.36, 0.68], [0.18, 0.74]], TICK), S([[0.86, 0.84], [0.96, 0.7]], TICK)]) },
  snake: { bits: 1, make: (b) => face(b[0], [S([[0.5, 0.03], [0.35, 0.15], [0.5, 0.26], [0.65, 0.15], [0.5, 0.03]], CUT), S([[0.5, 0.26], [0.3, 0.42], [0.68, 0.6], [0.34, 0.78], [0.62, 0.97]], ARC)]) },
  horse: { bits: 1, make: (b) => face(b[0], [S([[0.36, 0.4], [0.58, 0.36], [0.8, 0.42]], ARC), S([[0.36, 0.4], [0.24, 0.14], [0.08, 0.4]], CUT), S([[0.24, 0.14], [0.3, 0.03]], TICK), S([[0.3, 0.2], [0.41, 0.13]], TICK), S([[0.33, 0.28], [0.45, 0.21]], TICK), S([[0.36, 0.36], [0.48, 0.29]], TICK), S([[0.38, 0.42], [0.3, 0.93]], LEG), S([[0.45, 0.42], [0.5, 0.93]], LEG), S([[0.74, 0.44], [0.68, 0.93]], LEG), S([[0.8, 0.44], [0.86, 0.93]], LEG), S([[0.8, 0.42], [0.94, 0.52], [0.92, 0.8]], ARC, 0.85), S([[0.84, 0.47], [0.99, 0.7]], TICK)]) },
  goat: { bits: 1, make: (b) => face(b[0], [S([[0.34, 0.46], [0.56, 0.42], [0.78, 0.46]], ARC), S([[0.34, 0.46], [0.22, 0.3], [0.07, 0.4]], CUT), S([[0.22, 0.29], [0.3, 0.1], [0.45, 0.13], [0.43, 0.3]], ARC), S([[0.11, 0.42], [0.11, 0.6]], TICK), S([[0.36, 0.48], [0.32, 0.92]], LEG), S([[0.44, 0.48], [0.47, 0.92]], LEG), S([[0.7, 0.48], [0.67, 0.92]], LEG), S([[0.78, 0.47], [0.82, 0.92]], LEG), S([[0.78, 0.46], [0.89, 0.33]], TICK)]) },
  monkey: { bits: 1, make: (b) => face(b[0], [S(ell(0.36, 0.17, 0.12, 0.12, -100, 265, 12), ARC), S([[0.44, 0.26], [0.64, 0.4], [0.62, 0.66], [0.46, 0.8]], ARC), S([[0.48, 0.42], [0.27, 0.48], [0.18, 0.35]], ARC), S([[0.46, 0.8], [0.26, 0.74], [0.22, 0.93]], CUT), S([[0.56, 0.77], [0.8, 0.9], [0.95, 0.7], [0.85, 0.5], [0.75, 0.6]], ARC, 0.85)]) },
  rooster: { bits: 1, make: (b) => face(b[0], [S([[0.3, 0.42], [0.5, 0.34], [0.7, 0.46], [0.6, 0.66], [0.38, 0.64], [0.3, 0.42]], ARC), S([[0.33, 0.42], [0.24, 0.22]], CUT), S(ell(0.22, 0.16, 0.07, 0.07, -100, 265, 10), ARC, 0.85), S([[0.16, 0.1], [0.13, 0.01]], TICK), S([[0.22, 0.08], [0.22, 0.0]], TICK), S([[0.28, 0.1], [0.32, 0.02]], TICK), S([[0.15, 0.18], [0.03, 0.22]], TICK), S([[0.68, 0.46], [0.84, 0.2], [0.97, 0.24]], ARC), S([[0.69, 0.5], [0.9, 0.38], [0.99, 0.5]], ARC), S([[0.44, 0.65], [0.42, 0.9], [0.31, 0.95]], LEG), S([[0.54, 0.66], [0.57, 0.9], [0.68, 0.95]], LEG)]) },
  dog: { bits: 1, make: (b) => face(b[0], [S([[0.32, 0.44], [0.56, 0.42], [0.78, 0.44]], ARC), S([[0.32, 0.44], [0.22, 0.3], [0.05, 0.37]], CUT), S([[0.21, 0.3], [0.19, 0.13]], TICK), S([[0.27, 0.31], [0.31, 0.15]], TICK), S([[0.34, 0.46], [0.29, 0.92]], LEG), S([[0.41, 0.46], [0.45, 0.92]], LEG), S([[0.71, 0.46], [0.67, 0.92]], LEG), S([[0.78, 0.45], [0.83, 0.92]], LEG), S([[0.78, 0.44], [0.91, 0.3], [0.85, 0.13], [0.72, 0.2]], ARC, 0.85)]) },
  pig: { bits: 1, make: (b) => face(b[0], [S(ell(0.54, 0.52, 0.3, 0.2, -170, 190, 20), ARC), S([[0.26, 0.44], [0.08, 0.5], [0.08, 0.62], [0.26, 0.62]], CUT), S([[0.3, 0.36], [0.23, 0.24], [0.36, 0.32]], CUT, 0.85), S([[0.38, 0.7], [0.36, 0.9]], LEG), S([[0.48, 0.72], [0.48, 0.9]], LEG), S([[0.64, 0.72], [0.64, 0.9]], LEG), S([[0.74, 0.68], [0.76, 0.9]], LEG), S([[0.84, 0.48], [0.94, 0.42], [0.96, 0.54], [0.88, 0.55]], ARC, 0.8)]) },
  /* ---- the twelve signs of the ecliptic, their own symbols written with the brush ---- */
  aries: { bits: 0, make: () => [S([[0.12, 0.46], [0.1, 0.26], [0.24, 0.1], [0.42, 0.2], [0.5, 0.5], [0.5, 0.96]], ARC), S([[0.88, 0.46], [0.9, 0.26], [0.76, 0.1], [0.58, 0.2], [0.5, 0.5]], ARC)] },
  taurus: { bits: 0, make: () => [S(ell(0.5, 0.66, 0.25, 0.25, -100, 266, 20), ARC), S([[0.14, 0.05], [0.24, 0.3], [0.5, 0.4], [0.76, 0.3], [0.86, 0.05]], ARC)] },
  gemini: { bits: 0, make: () => [S([[0.1, 0.05], [0.5, 0.15], [0.9, 0.05]], ARC), S([[0.1, 0.95], [0.5, 0.85], [0.9, 0.95]], ARC), S([[0.34, 0.13], [0.34, 0.87]], CUT), S([[0.66, 0.13], [0.66, 0.87]], CUT)] },
  cancer: { bits: 0, make: () => { const up = [[0.92, 0.3], [0.6, 0.14], [0.28, 0.17], [0.13, 0.32], [0.27, 0.46], [0.41, 0.33], [0.3, 0.2]]; return [S(up, ARC), S(up.map(([x, y]) => [1 - x, 1 - y]), ARC)]; } },
  leo: { bits: 0, make: () => [S(ell(0.27, 0.6, 0.13, 0.13, -60, 300, 14), ARC), S([[0.33, 0.48], [0.3, 0.26], [0.5, 0.08], [0.72, 0.24], [0.68, 0.56], [0.66, 0.8], [0.8, 0.93], [0.94, 0.82]], ARC)] },
  virgo: { bits: 0, make: () => [S([[0.06, 0.2], [0.14, 0.12], [0.18, 0.26], [0.18, 0.8]], ARC), S([[0.18, 0.3], [0.3, 0.12], [0.4, 0.28], [0.4, 0.8]], ARC), S([[0.4, 0.3], [0.52, 0.12], [0.62, 0.28], [0.62, 0.72], [0.74, 0.92]], ARC), S([[0.62, 0.4], [0.8, 0.3], [0.92, 0.46], [0.8, 0.7], [0.52, 0.95]], ARC)] },
  libra: { bits: 0, make: () => [S([[0.08, 0.88], [0.92, 0.88]], CUT), S([[0.08, 0.64], [0.38, 0.64], [0.3, 0.5], [0.3, 0.32], [0.5, 0.15], [0.7, 0.32], [0.7, 0.5], [0.62, 0.64], [0.92, 0.64]], ARC)] },
  scorpio: { bits: 0, make: () => [S([[0.04, 0.2], [0.12, 0.12], [0.16, 0.26], [0.16, 0.8]], ARC), S([[0.16, 0.3], [0.28, 0.12], [0.38, 0.28], [0.38, 0.8]], ARC), S([[0.38, 0.3], [0.5, 0.12], [0.6, 0.28], [0.6, 0.74], [0.7, 0.9], [0.86, 0.86], [0.92, 0.64]], ARC), S([[0.8, 0.74], [0.92, 0.62]], TICK), S([[0.92, 0.62], [0.99, 0.78]], TICK)] },
  archer: { bits: 0, make: () => [S([[0.1, 0.92], [0.88, 0.12]], CUT), S([[0.54, 0.12], [0.88, 0.12]], CUT), S([[0.88, 0.12], [0.88, 0.46]], CUT), S([[0.22, 0.48], [0.54, 0.8]], CUT)] },
  capricorn: { bits: 0, make: () => [S([[0.05, 0.16], [0.18, 0.13], [0.32, 0.68]], CUT), S([[0.32, 0.68], [0.4, 0.3], [0.52, 0.14], [0.63, 0.3], [0.63, 0.62], [0.58, 0.84], [0.44, 0.95]], ARC), S(ell(0.79, 0.7, 0.14, 0.15, 180, 545, 14), ARC)] },
  aquarius: { bits: 0, make: () => [S([[0.05, 0.42], [0.2, 0.25], [0.35, 0.42], [0.5, 0.25], [0.65, 0.42], [0.8, 0.25], [0.95, 0.42]], CUT), S([[0.05, 0.74], [0.2, 0.57], [0.35, 0.74], [0.5, 0.57], [0.65, 0.74], [0.8, 0.57], [0.95, 0.74]], CUT)] },
  pisces: { bits: 0, make: () => [S([[0.14, 0.05], [0.34, 0.3], [0.34, 0.7], [0.14, 0.95]], ARC), S([[0.86, 0.05], [0.66, 0.3], [0.66, 0.7], [0.86, 0.95]], ARC), S([[0.2, 0.5], [0.8, 0.5]], CUT)] },
};
const ELEMENTS = ['sun', 'moon', 'peaks', 'water', 'fire', 'earth', 'metal', 'wood', 'person', 'rain', 'cloud', 'cowrie', 'skein', 'jar', 'arrow', 'chariot'];
const ANIMALS = ['rat', 'ox', 'tiger', 'rabbit', 'dragon', 'snake', 'horse', 'goat', 'monkey', 'rooster', 'dog', 'pig'];
const SIGNS = ['aries', 'taurus', 'gemini', 'cancer', 'leo', 'virgo', 'libra', 'scorpio', 'archer', 'capricorn', 'aquarius', 'pisces'];
const GLYPHS = ELEMENTS.concat(ANIMALS, SIGNS);
const GLYPH_LABEL = { sun: ['sun', '日'], moon: ['moon', '月'], peaks: ['mountain', '山'], water: ['water', '水'], fire: ['fire', '火'], earth: ['earth', '土'], metal: ['metal', '金'], wood: ['wood', '木'], person: ['person', '人'], rain: ['rain', '雨'], cloud: ['cloud', '云'], cowrie: ['cowrie', '贝'], skein: ['thread', '丝'], jar: ['jar', '酉'], arrow: ['arrow', '矢'], chariot: ['chariot', '车'],
  rat: ['rat', '鼠'], ox: ['ox', '牛'], tiger: ['tiger', '虎'], rabbit: ['rabbit', '兔'], dragon: ['dragon', '龙'], snake: ['snake', '蛇'], horse: ['horse', '马'], goat: ['goat', '羊'], monkey: ['monkey', '猴'], rooster: ['rooster', '鸡'], dog: ['dog', '狗'], pig: ['pig', '猪'],
  aries: ['Aries', '白羊'], taurus: ['Taurus', '金牛'], gemini: ['Gemini', '双子'], cancer: ['Cancer', '巨蟹'], leo: ['Leo', '狮子'], virgo: ['Virgo', '处女'], libra: ['Libra', '天秤'], scorpio: ['Scorpio', '天蝎'], archer: ['Sagittarius', '射手'], capricorn: ['Capricorn', '摩羯'], aquarius: ['Aquarius', '水瓶'], pisces: ['Pisces', '双鱼'] };
/* drawn from a later script, because the oracle bones have no form for them */
const NOT_ORACLE = ['metal'];

/* the talisman's written textures, kept: what fills a compartment */
const TEXTURES = ['cloud', 'zigzag', 'water', 'curls', 'loops', 'meander', 'dashes', 'hooks'];
const TEXTURE_LABEL = { cloud: 'cloud script', zigzag: 'thunder line', water: 'water', curls: 'curls', loops: 'chain', meander: 'meander', dashes: 'stitches', hooks: 'hooks' };

/* ---------- reading the id ---------- */
function reader(hex) {
  const all = S2.bits(hex, 256);
  let cur = 0;
  return { take: (k) => { let v = 0; for (let i = 0; i < k; i++) v = (v << 1) | all[cur++ % 256]; return v; }, bits: (n) => { const o = []; for (let i = 0; i < n; i++) o.push(all[cur++ % 256]); return o; }, used: () => cur };
}
function glyphOf(rd) {
  const name = GLYPHS[rd.take(8) % GLYPHS.length];
  return { name, strokes: GLYPH[name].make(rd.bits(GLYPH[name].bits)) };
}

/* ---------- drawing ---------- */
function drawer(o, p, w0) {
  return (strokes, brushSpec, X, Y, weight, col, pitch) => {
    let out = '';
    const pp = pitch || p;
    for (const st of strokes) {
      const P = st.pts.map(([x, y]) => [X(x), Y(y)]);
      const sp = Object.assign({}, brushSpec, st.brush || {});
      const base = {
        w: w0 * o.w * (weight || 1) * (st.weight || 1), r: (sp.r == null ? 0.3 : sp.r) * pp, over: sp.over == null ? 0.3 : sp.over, entry: sp.entry || 'point', entryLen: (sp.entryLen == null ? 0.6 : sp.entryLen) * pp, press0: sp.press0 == null ? 0.08 : sp.press0,
        exit: sp.exit || 'taper', tail: (sp.tail == null ? 1.2 : sp.tail) * pp, bulge: sp.even ? 0 : sp.bulge == null ? 0.14 : sp.bulge, bulgeSigma: sp.bulgeSigma, hmul: sp.hmul == null ? (sp.even ? 1 : 0.86) : sp.hmul, thin: 0,
        pulse: sp.even ? 0 : sp.pulse == null ? 0.22 : sp.pulse, mod: 0, unit: pp, seed: 1, curve: sp.curve || 'corner', tension: sp.tension || 4, tremor: null, step: pp / 4, spm: 5, qm: 6,
      };
      if (st.horiz === true) Object.assign(base, { entry: 'point', entryLen: 0.4 * pp, exit: 'blunt', pulse: 0, curve: 'corner' });
      if (st.horiz === false) Object.assign(base, { entry: 'round', press0: 0.2, exit: 'taper', tail: 0.7 * pp, pulse: 0, curve: 'corner' });
      out += B.shape(B.stroke(P, base), col);
    }
    return out.replace(/(\d+\.\d)\d+/g, '$1');
  };
}

const KINDS = ['block', 'tx', 'cell', 'script'];
const NAMES = { block: 'Block', tx: 'Transaction', cell: 'Cell', script: 'Script' };
const STATES = { pressed: 1, ink: 1, unpressed: 1 };
const CLASSES = ['ckb', 'dao', 'tokens', 'objects', 'identities', 'other'];
/* the glyph's box in units, on a page seal and on a row seal */
const GB = 5.4, GBR = 6.2;
const BAND = 1.55;
const RINGS = ['zig', 'wave', 'ticks', 'stitch', 'scales', 'key', 'rope', 'comb'];
/* the capacity gauge: twelve places round the ring, one large pearl for every decimal digit of
   the capacity in whole CKB (61 CKB: two; 1,204: four; 1,200,000: seven) */
const GAUGE = 11;
const digitsOf = (ckb) => { const n = BigInt(ckb); if (n < 0n) throw new Error('seal: negative capacity'); const d = n.toString().length; if (d > GAUGE) throw new Error(`seal: capacity ${n} CKB has more than ${GAUGE} digits`); return d; };

function ringStrokes(name, Rm, a, k, cx, cy, flip) {
  const P = (r, th) => [cx + r * Math.cos(th), cy + r * Math.sin(th)];
  const out = [];
  const step = TAU / k;
  if (name === 'zig') { const pts = []; for (let i = 0; i <= 2 * k + 1; i++) pts.push(P(Rm + (i % 2 ? a : -a), (i * step) / 2)); out.push({ pts, brush: { curve: 'corner', r: 0.05, over: 0, entry: 'point', entryLen: 0.3, exit: 'taper', tail: 0.4, pulse: 0, bulge: 0.2, bulgeSigma: 0.18, hmul: 1 } }); }
  else if (name === 'wave' || name === 'rope') { for (const s of name === 'rope' ? [1, -1] : [1]) { const pts = []; for (let i = 0; i <= 6 * k + 2; i++) { const th = (i * step) / 6; pts.push(P(Rm + s * a * Math.sin(k * th * 0.5), th)); } out.push({ pts, brush: { curve: 'spline', tension: 6, entry: 'point', entryLen: 0.3, exit: 'taper', tail: 0.5, pulse: 0, bulge: 0, hmul: 1 } }); } }
  else if (name === 'ticks' || name === 'comb') for (let i = 0; i < k; i++) { const th = i * step, short = name === 'comb' && i % 2; out.push({ pts: [P(Rm - a, th), P(Rm + (short ? 0 : a), th)], brush: TICK }); }
  else if (name === 'stitch') for (let i = 0; i < k; i++) { const th = i * step, r = Rm + (i % 2 ? a * 0.6 : -a * 0.6); out.push({ pts: [P(r, th - step * 0.32), P(r, th), P(r, th + step * 0.32)], brush: { curve: 'spline', tension: 6, entry: 'point', entryLen: 0.3, exit: 'taper', tail: 0.4, pulse: 0, bulge: 0, hmul: 1 } }); }
  else if (name === 'scales') for (let i = 0; i < k; i++) { const th = i * step, pts = []; for (let j = 0; j <= 6; j++) pts.push(P(Rm - a + 2 * a * Math.sin((Math.PI * j) / 6), th + (j / 6) * step * 0.94)); out.push({ pts, brush: { curve: 'spline', tension: 6, entry: 'point', entryLen: 0.25, exit: 'taper', tail: 0.35, pulse: 0, bulge: 0, hmul: 1 } }); }
  else if (name === 'key') for (let i = 0; i < k; i++) { const th = i * step, d = flip ? -1 : 1; out.push({ pts: [P(Rm - a, th), P(Rm + a, th), P(Rm + a, th + d * step * 0.55), P(Rm - a * 0.1, th + d * step * 0.55)], brush: { curve: 'corner', r: 0.05, over: 0, entry: 'point', entryLen: 0.25, exit: 'taper', tail: 0.35, pulse: 0, bulge: 0.2, bulgeSigma: 0.18, hmul: 1 } }); }
  return out;
}

/* ---------- counting rods ---------- */
/* one digit in a box (x, y, w, h): 1-5 standing rods; 6-9 a lying rod over 1-4 standing ones;
   0 a ring (the Song zero). Returns strokes; `pitch` is the distance between rods. */
const ROD = { curve: 'corner', entry: 'round', press0: 0.16, exit: 'taper', tail: 0.45, pulse: 0, hmul: 1 };
const ROD_FLAT = { curve: 'corner', entry: 'point', entryLen: 0.3, press0: 0.1, exit: 'blunt', pulse: 0, hmul: 1 };
function rodDigit(d, x, y, w, h, pitch) {
  if (d === 0) { const r = Math.min(w, h) * 0.26; return [{ pts: ell(x + w / 2, y + h / 2, r, r, -100, 266, 12), brush: ARC }]; }
  const over = d > 5, n = over ? d - 5 : d, out = [];
  const top = over ? y + h * 0.3 : y, x0 = x + w / 2 - ((n - 1) * pitch) / 2;
  for (let i = 0; i < n; i++) out.push({ pts: [[x0 + i * pitch, top], [x0 + i * pitch, y + h]], brush: ROD });
  if (over) { const half = Math.max(((n - 1) * pitch) / 2 + pitch * 0.55, pitch * 0.9); out.push({ pts: [[x + w / 2 - half, y + h * 0.06], [x + w / 2 + half, y + h * 0.06]], brush: ROD_FLAT }); }
  return out;
}
/* a count in a zone: its digits one under the other, the greatest on top. `maxDigits` is how
   many the zone can take; a greater count is a full block, which is not a number and cannot be
   mistaken for one. Returns { strokes, full } */
function rodCount(n, x, y, w, h, pitch, maxDigits, digitH) {
  if (!Number.isInteger(n) || n < 0) throw new Error('seal: a count is a whole number, got ' + n);
  const ds = String(n).split('').map(Number);
  if (ds.length > maxDigits) return { strokes: [], full: true };
  const gap = digitH * 0.42, total = ds.length * digitH + (ds.length - 1) * gap;
  let yy = y + (h - total) / 2;
  const strokes = [];
  for (const d of ds) { strokes.push(...rodDigit(d, x, yy, w, digitH, pitch)); yy += digitH + gap; }
  return { strokes, full: false };
}

/* the window round a cell's glyph: the class of its type script */
function windowOf(cls, cx, cy, r, col, sw) {
  const line = (d) => `<path d="${d}" fill="none" stroke="${col}" stroke-width="${f2(sw)}" stroke-linejoin="round"/>`;
  const circ = (rr) => `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f2(rr)}" fill="none" stroke="${col}" stroke-width="${f2(sw)}"/>`;
  switch (cls) {
    case 'ckb': return '';
    case 'other': return circ(r);
    case 'dao': return circ(r) + circ(r - sw * 2.4);
    case 'tokens': { const h = (r / Math.SQRT2) * 0.99; return `<rect x="${f2(cx - h)}" y="${f2(cy - h)}" width="${f2(2 * h)}" height="${f2(2 * h)}" rx="${f2(h * 0.08)}" fill="none" stroke="${col}" stroke-width="${f2(sw)}"/>`; }
    case 'objects': { let d = ''; for (let i = 0; i < 6; i++) { const a = -Math.PI / 2 + (i * Math.PI) / 3; d += (i ? 'L' : 'M') + f2(cx + r * 1.04 * Math.cos(a)) + ' ' + f2(cy + r * 1.04 * Math.sin(a)); } return line(d + 'Z'); }
    case 'identities': { const hw = r * 0.7, yb = cy + r * 0.72, ys = cy - r * 0.12; return line(`M${f2(cx - hw)} ${f2(yb)}L${f2(cx - hw)} ${f2(ys)}A${f2(hw)} ${f2(hw)} 0 0 1 ${f2(cx + hw)} ${f2(ys)}L${f2(cx + hw)} ${f2(yb)}Z`); }
    default: throw new Error('seal: no such class ' + cls);
  }
}

/* ---------- the round core ---------- */
/* the rim of a round core, read from the transaction's hash; a cell cut from that core keeps it.
   Song dishes and mirrors: plain, eight-sided, the mallow mouth (six nicks), the caltrop
   flower (eight points), the flower mouth (twelve scallops). r / R for an angle from the top. */
const RIMS = ['round', 'oct', 'mallow6', 'caltrop8', 'scallop12'];
const RIM_LABEL = { round: ['plain', '圆口'], oct: ['eight-sided', '八方'], mallow6: ['mallow mouth', '葵口'], caltrop8: ['caltrop flower', '菱花'], scallop12: ['flower mouth', '花口'] };
const RIM_MIN = 0.915;
function rimR(shape, th) {
  switch (shape) {
    case 'round': return 1;
    case 'oct': { const a = Math.PI / 4, m = (((th % a) + a) % a) - a / 2; return Math.cos(a / 2) / Math.cos(m); }
    case 'mallow4': case 'mallow6': { const a = TAU / (shape === 'mallow4' ? 4 : 6), d = Math.abs((((th % a) + a) % a) - a / 2); return 1 - 0.085 * Math.exp(-Math.pow(d / 0.075, 2)); }
    case 'caltrop8': return 0.915 + 0.085 * Math.pow(Math.abs(Math.cos(4 * th)), 1.3);
    case 'scallop12': return 0.95 + 0.05 * Math.abs(Math.sin(6 * th));
    default: throw new Error('seal: no such rim ' + shape);
  }
}
function rimPath(shape, cx, cy, R) {
  let d = '';
  const N = 180;
  for (let i = 0; i < N; i++) { const th = (i * TAU) / N, r = R * rimR(shape, th); d += (i ? 'L' : 'M') + f2(cx + r * Math.sin(th)) + ' ' + f2(cy - r * Math.cos(th)); }
  return d + 'Z';
}
/* what a transaction's core is made of, read in one order by the transaction and by its cells */
function coreOf(hex) {
  const rd = reader(hex), g = glyphOf(rd);
  return { rd, g, ring: RINGS[rd.take(3)], k: rd.take(1) ? 24 : 20, flip: rd.take(1), rim: RIMS[rd.take(8) % RIMS.length] };
}
/* keep the parts of strokes outside the given angular gaps (centre cx, cy; gaps as [mid, half]) */
function cutGaps(strokes, cx, cy, gaps) {
  const inGap = ([x, y]) => { const a = Math.atan2(y - cy, x - cx); return gaps.some(([mid, half]) => { let d = a - mid; while (d > Math.PI) d -= TAU; while (d < -Math.PI) d += TAU; return Math.abs(d) < half; }); };
  const out = [];
  for (const st of strokes) {
    let run = [];
    for (const pt of st.pts) { if (inGap(pt)) { if (run.length > 1) out.push(Object.assign({}, st, { pts: run })); run = []; } else run.push(pt); }
    if (run.length > 1) out.push(Object.assign({}, st, { pts: run }));
  }
  return out;
}
/* the twelve hours of a ring: hour 0 at the top, clockwise */
const hourAngle = (h) => -Math.PI / 2 + (h * TAU) / 12;
/* the owner's hour: eleven of the twelve (the sixth, at the foot, is where a cell's index is written) */
const ownerOf = (hex) => { const rd = reader(hex), g = glyphOf(rd), v = rd.take(8) % 11; return { g, hour: v >= 6 ? v + 1 : v }; };

/* seal(kind, id, size, o) — size is the seal's height in px
     block:  { hash, txs }               txs = how many transactions it holds
     tx:     { hash, inputs, outputs }   inputs = cells consumed (0 for a cellbase)
     cell:   { tx, index, lock, type: { class, hash }, capacity }
               tx, index = its out point; lock = the lock script's hash; type.hash = the type
               script's hash, null when the cell has none (class 'ckb'); capacity in whole CKB
     script: the 32-byte script hash
     o.form 'page' (red text) | 'row' (white text); o.state 'pressed' | 'ink' | 'unpressed' */
function seal(kind, id, size, o) {
  o = Object.assign({ form: 'page', state: 'pressed', ink: '#c2452d', ground: '#15120f', dead: '#8a8073', tilt: 0, ratio: 0.58, w: 0.84, framew: 2.2, pad: 1.0, uid: 's' }, o || {});
  if (!KINDS.includes(kind)) throw new Error('seal: no such kind ' + kind);
  if (!STATES[o.state]) throw new Error('seal: no such state ' + o.state);
  const row = o.form === 'row';
  const w0 = row ? 0.7 : o.ratio, p = 1 + w0;
  const col = o.state === 'ink' ? o.dead : o.ink;
  const hollow = o.state === 'unpressed';
  const DASH = ` stroke-dasharray="${row ? '1.5 1.1' : '1.9 1.4'}"`;
  const draw = drawer(o, p, w0);
  const chosen = [];
  let W, H, body = '', vx = 0, vy = 0;
  const fc = row && !hollow ? o.ground : col;
  const glyphAt = (g, bx, by, s, c, weight, pitch) => draw(g.strokes, CUT, (x) => bx + x * s, (y) => by + y * s, weight, c, pitch);
  const hair = 0.6;
  const RULE = { curve: 'corner', r: 0.2, over: 0.1, entry: 'point', exit: 'taper' };
  const textureIn = (rd, R, ox, oy, m) => {
    let idx = rd.take(3);
    if (TEXTURES[idx] === textureIn.last) idx = (idx + 1) % TEXTURES.length;
    const name = TEXTURES[idx]; textureIn.last = name; chosen.push(name);
    const pat = B.REGION[name];
    const rw = Math.max(1, Math.round(R.w - 2 * m)), rh = Math.max(1, Math.round(R.h - 2 * m));
    let strokes = pat.make(rd.bits(pat.bits(rw, rh)), rw, rh);
    if (!pat.brush.even) strokes = B.orderStrokes(strokes);
    const sx = (R.w - 2 * m) / rw, sy = (R.h - 2 * m) / rh;
    return draw(strokes, pat.brush, (x) => ox + (R.x + m + x * sx) * p, (yy) => oy + (R.y + m + yy * sy) * p, 0.92, col);
  };
  const rimStroke = (shape, cx, cy, R, c, sw, dash) => shape === 'round'
    ? `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f2(R)}" fill="none" stroke="${c}" stroke-width="${f2(sw)}"${dash || ''}/>`
    : `<path d="${rimPath(shape, cx, cy, R)}" fill="none" stroke="${c}" stroke-width="${f2(sw)}" stroke-linejoin="round"${dash || ''}/>`;
  const rimFill = (shape, cx, cy, R, c) => shape === 'round' ? `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f2(R)}" fill="${c}"/>` : `<path d="${rimPath(shape, cx, cy, R)}" fill="${c}"/>`;
  let hex;

  if (kind === 'block') {
    hex = id.hash;
    const rd = reader(hex), g = glyphOf(rd);
    chosen.push(g.name);
    if (row) {
      /* the glyph, and under a line the count in rods (to nine; a full block beyond) */
      const s = 5.2, side = 8.8;
      W = H = side;
      body += hollow ? `<rect x="${hair / 2}" y="${hair / 2}" width="${f1(side - hair)}" height="${f1(side - hair)}" rx="0.6" fill="none" stroke="${col}" stroke-width="${hair}"${DASH}/>` : `<rect width="${f1(side)}" height="${f1(side)}" rx="0.6" fill="${col}"/>`;
      body += glyphAt(g, (side - s) / 2, 0.5, s, fc, 1.15, p * 0.9);
      body += `<path d="M0.9 6.15L${f1(side - 0.9)} 6.15" stroke="${fc}" stroke-width="0.32"/>`;
      if (id.txs != null) {
        const r = rodCount(id.txs, 0.9, 6.55, side - 1.8, 1.75, 0.95, 1, 1.75);
        body += r.full ? `<rect x="${f2(side / 2 - 2.2)}" y="6.65" width="4.4" height="1.55" fill="${fc}"/>` : draw(r.strokes, ROD, (xx) => xx, (yy) => yy, 0.85, fc, p * 0.6);
      }
    } else {
      const FW = 11.4, FH = 11.4, gap = 0.9;
      const wf = w0 * o.framew, pad = o.pad;
      W = H = 2 * wf + 2 * pad + FW * p;
      const ox = wf + pad, oy = wf + pad;
      const gw = GB + 0.2, ow = FW - gw - gap;
      const right = rd.take(1), top = rd.take(1);
      const gx = right ? FW - gw : 0, tx0 = right ? 0 : gw + gap;
      const gy = top ? 0 : FH - GB;
      let field = glyphAt(g, ox + (gx + 0.1) * p, oy + gy * p, GB * p, col, 1.3, p * 1.25);
      /* beside the glyph, in its own column: how many transactions, digit under digit */
      const cz = { x: gx, y: top ? GB + gap : 0, w: gw, h: FH - GB - gap };
      if (id.txs != null) {
        const nd = String(id.txs).length;
        const r = rodCount(id.txs, ox + cz.x * p, oy + (cz.y + 0.35) * p, cz.w * p, (cz.h - 0.7) * p, 0.66 * p, 4, [4.4, 2.9, 1.95, 1.4][nd - 1] * p);
        if (r.full) throw new Error(`seal: ${id.txs} transactions do not fit four digits`);
        field += draw(r.strokes, ROD, (xx) => xx, (yy) => yy, 1.05, col, p);
      }
      const regions = [];
      const nb = 2 + rd.take(1), hs = [];
      for (let i = 0; i < nb; i++) hs.push([3, 2, 3, 2][(i + rd.take(1)) % 4]);
      const scale = (FH - (nb - 1) * gap) / hs.reduce((a, c) => a + c, 0);
      let y = 0;
      const bars = [];
      for (let i = 0; i < nb; i++) { const h = hs[i] * scale; regions.push({ x: tx0, y, w: ow, h }); y += h; if (i < nb - 1) { bars.push({ pts: [[tx0 + 0.3, y + gap / 2], [tx0 + ow - 0.3, y + gap / 2]], horiz: true }); y += gap; } }
      bars.push({ pts: [[gx + 0.3, top ? GB + gap / 2 : FH - GB - gap / 2], [gx + gw - 0.3, top ? GB + gap / 2 : FH - GB - gap / 2]], horiz: true });
      const vxr = right ? gx - gap / 2 : gw + gap / 2;
      bars.push({ pts: [[vxr, 0.1], [vxr, FH - 0.1]], horiz: false, weight: 0.8 });
      for (const R of regions) field += textureIn(rd, R, ox, oy, 0.25);
      field += draw(bars, RULE, (x) => ox + x * p, (yy) => oy + yy * p, 0.85, col);
      if (hollow) field = `<g opacity="0.5">${field}</g>`;
      body += `<rect x="${f1(wf / 2)}" y="${f1(wf / 2)}" width="${f1(W - wf)}" height="${f1(H - wf)}" rx="0.7" fill="none" stroke="${col}" stroke-width="${f1(hollow ? wf * 0.4 : wf)}"${hollow ? DASH : ''}/>` + field;
    }
  } else if (kind === 'script') {
    hex = id;
    const rd = reader(hex), g = glyphOf(rd);
    chosen.push(g.name);
    if (row) {
      const s = GBR, bw = s + 1.6, bh = s + 4.2;
      W = bw; H = bh;
      body += hollow ? `<rect x="${hair / 2}" y="${hair / 2}" width="${f1(bw - hair)}" height="${f1(bh - hair)}" rx="${f1(bw * 0.36)}" fill="none" stroke="${col}" stroke-width="${hair}"${DASH}/>` : `<rect width="${f1(bw)}" height="${f1(bh)}" rx="${f1(bw * 0.36)}" fill="${col}"/>`;
      body += glyphAt(g, 0.8, (bh - s) / 2, s, fc, 1.2, p);
    } else {
      const FW = GB + 0.4, gap = 0.9, th = 3.2, FH = GB + gap + th;
      const wf = w0 * 1.3, pad = 1.7;
      W = 2 * wf + 2 * pad + FW * p; H = 2 * wf + 2 * pad + FH * p;
      const ox = wf + pad, oy = wf + pad, top = rd.take(1);
      rd.take(8); /* the owner's hour, read by its cells */
      let field = glyphAt(g, ox + 0.2 * p, oy + (top ? 0 : th + gap) * p, GB * p, col, 1.3, p * 1.25);
      field += textureIn(rd, { x: 0, y: top ? GB + gap : 0, w: FW, h: th }, ox, oy, 0.2);
      field += draw([{ pts: [[0.3, top ? GB + gap / 2 : th + gap / 2], [FW - 0.3, top ? GB + gap / 2 : th + gap / 2]], horiz: true }], RULE, (x) => ox + x * p, (yy) => oy + yy * p, 0.85, col);
      if (hollow) field = `<g opacity="0.5">${field}</g>`;
      body += `<rect x="${f1(wf / 2)}" y="${f1(wf / 2)}" width="${f1(W - wf)}" height="${f1(H - wf)}" rx="${f1(W * 0.34)}" fill="none" stroke="${col}" stroke-width="${f1(hollow ? wf * 0.4 : wf)}"${hollow ? DASH : ''}/>` + field;
    }
  } else if (kind === 'tx') {
    hex = id.hash;
    const core = coreOf(hex), g = core.g;
    chosen.push(g.name, core.ring, core.rim);
    const counts = [id.inputs, id.outputs];
    if (row) {
      const s = GBR, rc = ((s / 2) * Math.SQRT2 * 0.9 + 0.3) / RIM_MIN, bh = 2 * (rc + 0.55), zw = 5.3, bw = bh + 2 * zw;
      W = bw; H = bh;
      body += hollow ? `<rect x="${hair / 2}" y="${hair / 2}" width="${f1(bw - hair)}" height="${f1(bh - hair)}" rx="0.6" fill="none" stroke="${col}" stroke-width="${hair}"${DASH}/>` : `<rect width="${f1(bw)}" height="${f1(bh)}" rx="0.6" fill="${col}"/>`;
      body += rimStroke(core.rim, bw / 2, bh / 2, rc, fc, 0.5);
      body += glyphAt(g, bw / 2 - s / 2, bh / 2 - s / 2, s, fc, 1.2, p);
      counts.forEach((n, i) => {
        if (n == null) return; /* not known (a transaction the store does not hold): the end stays empty */
        const x = i ? bw / 2 + rc + 0.25 : 0.55, zwi = bw / 2 - rc - 0.8;
        const r = rodCount(n, x, 1.5, zwi, bh - 3, 0.98, 1, 5.6); /* a row seal counts to nine */
        if (r.full) body += `<rect x="${f2(x + zwi * 0.2)}" y="${f2(bh * 0.24)}" width="${f2(zwi * 0.6)}" height="${f2(bh * 0.52)}" fill="${fc}"/>`;
        else body += draw(r.strokes, ROD, (xx) => xx, (yy) => yy, 0.95, fc, p * 0.8);
      });
    } else {
      const s = GB * p, rIn = (s / 2) * Math.SQRT2 * 0.88, band = BAND * p;
      const rimw = w0 * 1.3, Rout = (rIn + band + 0.15 * p + rimw / 2) / RIM_MIN;
      const wf = w0 * o.framew, fgap = 0.2 * p, zw = 4.3 * p;
      H = 2 * (Rout + fgap + wf); W = H + 2 * zw;
      const cx = W / 2, cy = H / 2;
      const Rm = rIn + band / 2 + 0.05 * p;
      let coreSvg = draw(ringStrokes(core.ring, Rm, band * 0.3, core.k, cx, cy, core.flip), CUT, (x) => x, (y) => y, 0.8, col, p);
      coreSvg += `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f1(rIn + 0.1 * p)}" fill="none" stroke="${col}" stroke-width="${f1(w0 * 0.32)}"/>`;
      coreSvg += glyphAt(g, cx - s / 2, cy - s / 2, s, col, 1.3, p * 1.25);
      counts.forEach((n, i) => {
        if (n == null) return;
        const zx = i ? cx + Rout + 0.15 * p : wf + 0.5 * p, zwi = cx - Rout - wf - 0.65 * p;
        const nd = String(n).length;
        const r = rodCount(n, zx, wf + 0.9 * p, zwi, H - 2 * wf - 1.8 * p, 0.66 * p, 4, [4.2, 3.0, 2.05, 1.5][Math.min(nd, 4) - 1] * p);
        if (r.full) throw new Error(`seal: ${n} cells do not fit four digits`);
        coreSvg += draw(r.strokes, ROD, (xx) => xx, (yy) => yy, 1.05, col, p);
      });
      if (hollow) coreSvg = `<g opacity="0.5">${coreSvg}</g>`;
      body += `<rect x="${f1(wf / 2)}" y="${f1(wf / 2)}" width="${f1(W - wf)}" height="${f1(H - wf)}" rx="0.7" fill="none" stroke="${col}" stroke-width="${f1(hollow ? wf * 0.4 : wf)}"${hollow ? DASH : ''}/>`;
      body += rimStroke(core.rim, cx, cy, Rout - rimw / 2, col, hollow ? rimw * 0.4 : rimw) + coreSvg;
    }
  } else {
    /* the cell. From its out point: the rim and the written border of its transaction's core,
       and its index in rods at the foot. From its fields: the glyph of its type script's hash in
       the window of the type's class (none of either when it has no type); the pearls of its
       capacity; its owner's small stone, pressed at the owner's hour, where the pearls start. */
    hex = id.tx;
    if (!CLASSES.includes(id.type.class)) throw new Error('seal: no such class ' + id.type.class);
    if ((id.type.hash == null) !== (id.type.class === 'ckb')) throw new Error('seal: a cell has a type hash exactly when its class is not ckb');
    if (!Number.isInteger(id.index) || id.index < 0) throw new Error('seal: a cell index is a whole number, got ' + id.index);
    const core = coreOf(hex), own = ownerOf(id.lock), nd = digitsOf(id.capacity);
    const tg = id.type.hash ? glyphOf(reader(id.type.hash)) : null;
    chosen.push(tg ? tg.name : '-', core.rim, core.ring, own.g.name);
    const ha = hourAngle(own.hour);
    if (row) {
      const s = 6.0, rw = (s / 2) * Math.SQRT2 * 1.02, band = 1.15, Rm = rw + 0.28 + band / 2, R = (rw + 0.28 + band + 0.2) / RIM_MIN;
      W = H = 2 * R;
      const c = R;
      body += hollow ? rimStroke(core.rim, c, c, R - hair / 2, col, hair, DASH) : rimFill(core.rim, c, c, R, col);
      for (let j = 1; j <= nd; j++) { const a = hourAngle(own.hour + j); body += `<circle cx="${f2(c + Rm * Math.cos(a))}" cy="${f2(c + Rm * Math.sin(a))}" r="0.4" fill="${fc}"/>`; }
      /* the owner's stone, as small as it goes: an upright mark at the owner's hour */
      body += `<rect x="${f2(c + Rm * Math.cos(ha) - 0.52)}" y="${f2(c + Rm * Math.sin(ha) - 0.74)}" width="1.04" height="1.48" rx="0.36" fill="${fc}"/>`;
      if (tg) { body += windowOf(id.type.class, c, c, rw + 0.1, fc, 0.42); body += glyphAt(tg, c - s / 2, c - s / 2 + (id.type.class === 'identities' ? 0.25 : 0), s, fc, 1.15, p * 0.9); }
    } else {
      const s = GB * p * 0.92, rw = (s / 2) * Math.SQRT2 * 1.02, bb = 1.4 * p;
      const RmB = rw + 0.25 * p + bb / 2;
      const rimw = w0 * 1.4, Rr = (RmB + bb / 2 + 0.2 * p + rimw / 2) / RIM_MIN; /* the rim's centre line, at its widest */
      const m = 1.25 * p; /* room for the beads and for the owner's stone where it overhangs the rim */
      W = H = 2 * (Rr + rimw / 2 + m);
      const c = W / 2;
      /* the index, in rods, where the border opens at the foot */
      const ds = String(id.index).split('').map(Number), pitch = 0.46 * p, dh = bb * 0.8;
      const widths = ds.map((d) => (d === 0 ? 1.1 : (d > 5 ? Math.max(d - 5, 2) : d)) * pitch + 0.1 * p), dgap = 0.55 * p;
      const total = widths.reduce((a2, b2) => a2 + b2, 0) + (ds.length - 1) * dgap;
      let idx = '', xx = c - total / 2;
      ds.forEach((d, i) => { const xc = xx + widths[i] / 2, dy = Math.sqrt(Math.max(0, RmB * RmB - (xc - c) * (xc - c))); idx += draw(rodDigit(d, xx, c + dy - dh / 2, widths[i], dh, pitch), ROD, (x) => x, (y) => y, 0.95, col, p * 0.8); xx += widths[i] + dgap; });
      /* the owner's stone, on the border at the owner's hour */
      const cw = 2.9 * p, ch = 3.9 * p, qr = RmB + 0.45 * p, qx = c + qr * Math.cos(ha), qy = c + qr * Math.sin(ha);
      const gaps = [[Math.PI / 2, (total / 2 + 0.55 * p) / RmB], [ha, (cw * 0.7) / RmB]];
      let coreSvg = draw(cutGaps(ringStrokes(core.ring, RmB, bb * 0.3, core.k, c, c, core.flip), c, c, gaps), CUT, (x) => x, (y) => y, 0.8, col, p);
      coreSvg += idx;
      if (tg) { coreSvg += windowOf(id.type.class, c, c, rw + 0.05 * p, col, w0 * 0.4); coreSvg += glyphAt(tg, c - s / 2, c - s / 2 + (id.type.class === 'identities' ? 0.04 * s : 0), s, col, 1.3, p * 1.25); }
      if (hollow) coreSvg = `<g opacity="0.5">${coreSvg}</g>`;
      body += rimStroke(core.rim, c, c, Rr, col, hollow ? rimw * 0.4 : rimw, hollow ? DASH : '') + coreSvg;
      /* the capacity: a bead on the rim for each digit, clockwise from the owner's stone */
      for (let j = 1; j <= nd; j++) { const h = own.hour + j, a = hourAngle(h), rr = Rr * rimR(core.rim, (h * TAU) / 12); body += `<circle cx="${f2(c + rr * Math.cos(a))}" cy="${f2(c + rr * Math.sin(a))}" r="${f2(0.5 * p)}" fill="${hollow ? o.ground : col}" stroke="${hollow ? col : o.ground}" stroke-width="${f2(hollow ? 0.3 : 0.16 * p)}" paint-order="stroke"/>`; }
      const gs = cw * 0.8;
      body += `<rect x="${f2(qx - cw / 2)}" y="${f2(qy - ch / 2)}" width="${f2(cw)}" height="${f2(ch)}" rx="${f2(cw * 0.36)}" fill="${hollow ? o.ground : col}" stroke="${hollow ? col : o.ground}" stroke-width="${f2(hollow ? 0.35 : 0.3 * p)}"${hollow ? DASH : ''} paint-order="stroke"/>`;
      body += glyphAt(own.g, qx - gs / 2, qy - gs / 2, gs, hollow ? col : o.ground, 0.66, p * 0.55);
    }
  }
  const scale = size / H;
  /* pressed across a seam: the gap between the two sheets shows through, seamPx wide */
  if (o.seamPx) body += `<path d="M${f2(W / 2)} -1L${f2(W / 2)} ${f1(H + 1)}" stroke="${o.seamColor || o.ground}" stroke-width="${f2(o.seamPx / scale)}"/>`;
  const pxW = Math.round(W * scale * 10) / 10, pxH = Math.round(H * scale * 10) / 10;
  const label = kind === 'tx' ? `Transaction seal ${hex.slice(0, 10)}, ${id.inputs} consumed, ${id.outputs} created` : kind === 'cell' ? `Cell seal ${hex.slice(0, 10)}:${id.index}, ${id.type.class}, ${id.capacity} CKB` : kind === 'block' ? `Block seal ${hex.slice(0, 10)}, ${id.txs} transactions` : `Script seal ${hex.slice(0, 10)}`;
  const svg = `<svg width="${pxW}" height="${pxH}" viewBox="${f1(vx)} ${f1(vy)} ${f1(W)} ${f1(H)}" role="img" aria-label="${label}"${o.tilt ? ` style="transform: rotate(${o.tilt}deg)"` : ''}>${body}</svg>`;
  return o.meta ? { svg, chosen, pxW, pxH } : svg;
}

/* a count alone, for the legend: the rods of n in a box px tall */
function rods(n, px, col) {
  const p = 1.58, nd = String(n).length, dh = 4.2 * p, zw = 5 * 0.66 * p + 1.2, gap = dh * 0.42, hh = nd * dh + (nd - 1) * gap, m = 0.5;
  const r = rodCount(n, m, m, zw, hh, 0.66 * p, 4, dh);
  const g = drawer({ w: 0.84 }, p, 0.58)(r.strokes, ROD, (x) => x, (y) => y, 1.05, col, p);
  const W = zw + 2 * m, H = hh + 2 * m, k = px / (dh + 2 * m);
  return `<svg width="${Math.round(W * k * 10) / 10}" height="${Math.round(H * k * 10) / 10}" viewBox="0 0 ${f1(W)} ${f1(H)}" role="img" aria-label="${n} in counting rods">${g}</svg>`;
}

/* one glyph alone (bits given), for the lexicon */
function specimen(name, bits, px, col) {
  const strokes = GLYPH[name].make(bits.slice(0, GLYPH[name].bits));
  const p = 1.58, s = GB * p, m = 0.6;
  const g = drawer({ w: 0.84 }, p, 0.58)(strokes, CUT, (x) => m + x * s, (y) => m + y * s, 1.3, col, p * 1.25);
  const W = 2 * m + s;
  return `<svg width="${px}" height="${px}" viewBox="0 0 ${f1(W)} ${f1(W)}" role="img" aria-label="${name}">${g}</svg>`;
}
/* one texture alone */
function texture(name, bits, px, col) {
  const pat = B.REGION[name], w = 5, h = 3, p = 1.58, m = 0.5;
  const b = []; for (let i = 0; i < pat.bits(w, h); i++) b.push(bits[i % bits.length]);
  let strokes = pat.make(b, w, h);
  if (!pat.brush.even) strokes = B.orderStrokes(strokes);
  const g = drawer({ w: 0.84 }, p, 0.58)(strokes, pat.brush, (x) => m + x * p, (y) => m + y * p, 0.92, col);
  const W = 2 * m + w * p, H = 2 * m + h * p;
  return `<svg width="${Math.round((px * W) / H)}" height="${px}" viewBox="0 0 ${f1(W)} ${f1(H)}" role="img" aria-label="${name}">${g}</svg>`;
}
/* which glyph a hash gives (for choosing the board's examples) */
const glyphName = (hex) => glyphOf(reader(hex)).name;
const rimName = (hex) => coreOf(hex).rim;
module.exports = { seal, rods, specimen, texture, glyphName, rimName, GLYPH, GLYPHS, ELEMENTS, ANIMALS, SIGNS, GLYPH_LABEL, NOT_ORACLE, TEXTURES, TEXTURE_LABEL, RINGS, RIMS, RIM_LABEL, CLASSES, GAUGE };
