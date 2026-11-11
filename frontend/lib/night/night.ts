/**
 * The night of the not-found and error pages, direction Z: every block the new hash is poured in
 * as what it is — 256 bits, a 16 × 16 grid of ink, 1 jade and 0 violet — that holds for a second,
 * then dissolves in a current the hash itself seeded. The 1-bits leave as motes whose short trails
 * are the one sharp line over the soft ink. Nothing is depicted.
 *
 * One WebGL2 canvas. `createNight` returns a handle whose `pour(hash)` is called for every new
 * block; the page's own hash (or nothing) is poured first. When the browser takes the context away
 * the handle reports `lost` and the page shows its CSS night; when it comes back everything is
 * rebuilt. Off screen for a moment, the night hands its context back on its own.
 */

import { bitsOf, gridCell, seedOf, type Grid } from '@/lib/night/bits';
import type { NightColors } from '@/lib/night/colors';
import { Current, FIELD_H, FIELD_W } from '@/lib/night/field';
import {
  bindTex,
  deleteTarget,
  drawTriangle,
  FADE,
  INK_STEP,
  points,
  program,
  setColors,
  SHOW,
  target,
  texture,
  uniforms,
  type Points,
  type Target,
} from '@/lib/night/gl';
import { Motes } from '@/lib/night/motes';

export type NightState = 'running' | 'still' | 'lost' | 'failed';

export interface NightOptions {
  /** The first grid: the hash this page is about, or null for no grid until the first block. */
  hash: string | null;
  grid: Grid;
  /** The theme's colours (lib/night/colors.ts): the base, the four hues, night or day. */
  colors: NightColors;
  /** Half-resolution ink and trails (phones). */
  half?: boolean;
  /** Reduced motion: one frame, nothing moves, no ticks. */
  still?: boolean;
  onState?: (state: NightState) => void;
}

export interface Night {
  /** A new block: its hash is poured as the next grid and seeds the next current. */
  pour(hash: string): void;
  stop(): void;
}

const INK_DECAY = 0.9975; // per frame at 60 Hz: 37 % after 6.6 s, a grid gone about when the next lands
const INK_ADV = 0.0022; // uv per frame at 60 Hz: ~2 px at 1280
const INK_STIR = 0.6;
const HOLD_SECONDS = 0.9;
const TRAIL_FADE = 0.955;
const MOTE_SPEED = 0.0024;
const RELEASE_AFTER_MS = 1500;

/** The lose-context handle of every canvas a night ran on: a lost context cannot hand it out again. */
const loseExtensions = new WeakMap<HTMLCanvasElement, WEBGL_lose_context>();

interface Scene {
  ink: { a: Target; b: Target; w: number; h: number };
  trails: { a: Target; b: Target; w: number; h: number };
  inkStep: WebGLProgram;
  fade: WebGLProgram;
  show: WebGLProgram;
  stamp: Points;
  motePoints: Points;
  fieldA: WebGLTexture;
  fieldB: WebGLTexture;
}

export function createNight(canvas: HTMLCanvasElement, opts: NightOptions): Night {
  const report = (state: NightState) => opts.onState?.(state);
  const stopped = () => ({ pour() {}, stop() {} });
  const gl = canvas.getContext('webgl2', {
    antialias: false,
    alpha: false,
    preserveDrawingBuffer: false,
  });
  if (!gl) {
    report('failed');
    return stopped();
  }
  const W = (canvas.width = canvas.clientWidth || canvas.width || 1);
  const H = (canvas.height = canvas.clientHeight || canvas.height || 1);
  const aspect = W / H;
  const half = !!opts.half;
  const grid = opts.grid;
  const colors = opts.colors;
  const scene = canvas.parentElement ?? canvas;
  const t0 = performance.now();
  let lastT = t0;
  const s = {
    t: 0,
    dt: 1 / 60,
    mouse: [0.5, 0.5] as [number, number],
    mvel: [0, 0] as [number, number],
    moved: false,
    sincePour: 20,
  };
  let hash = opts.hash;
  const current = new Current(hash ? seedOf(hash) : 0.3, aspect);
  const motes = new Motes(1.0);

  let sc: Scene | null = null;
  const build = (): Scene => {
    const iw = half ? W >> 1 : W;
    const ih = half ? H >> 1 : H;
    const built: Scene = {
      ink: { a: target(gl, iw, ih, 'f16'), b: target(gl, iw, ih, 'f16'), w: iw, h: ih },
      trails: { a: target(gl, iw, ih, 'f16'), b: target(gl, iw, ih, 'f16'), w: iw, h: ih },
      inkStep: program(gl, INK_STEP),
      fade: program(gl, FADE),
      show: program(gl, SHOW),
      stamp: points(gl, 256, half ? grid.pitch * 0.32 : grid.pitch * 0.64, colors),
      motePoints: points(gl, motes.N, half ? 1.1 : 2.0, colors),
      fieldA: texture(gl, FIELD_W, FIELD_H, 'u8', null, gl.REPEAT),
      fieldB: texture(gl, FIELD_W, FIELD_H, 'u8', null, gl.REPEAT),
    };
    setColors(gl, built.show, colors);
    current.dirty = true;
    if (hash) stampGrid(built, bitsOf(hash));
    else stain(built);
    return built;
  };
  const uploadField = (c: Scene) => {
    if (!current.dirty) return;
    for (const [tex, data] of [
      [c.fieldA, current.a],
      [c.fieldB, current.b],
    ] as const) {
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA8,
        FIELD_W,
        FIELD_H,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        data
      );
    }
    current.dirty = false;
  };
  const bindField = (c: Scene, p: WebGLProgram) => {
    bindTex(gl, p, 'u_fA', c.fieldA, 1);
    bindTex(gl, p, 'u_fB', c.fieldB, 2);
    uniforms(gl, p, { u_blend: current.blend, u_t: current.t });
  };
  /** The grid of ink: 1 jade at .9, 0 violet at .45. */
  const stampGrid = (c: Scene, bits: Uint8Array) => {
    for (let i = 0; i < 256; i++) {
      const [x, y] = gridCell(grid, i, W, H);
      c.stamp.pos[i * 2] = x;
      c.stamp.pos[i * 2 + 1] = y;
      c.stamp.col[i] = bits[i] ? 0 : 1.5;
      c.stamp.alp[i] = bits[i] ? 0.9 : 0.45;
    }
    c.stamp.draw(c.ink.a.f, c.ink.w, c.ink.h, 1);
  };
  /** No hash yet: a faint violet stain where the first fact will land, for the current to work on. */
  const stain = (c: Scene) => {
    c.stamp.pos[0] = grid.cx;
    c.stamp.pos[1] = grid.cy;
    c.stamp.col[0] = 1.5;
    c.stamp.alp[0] = 0.1;
    c.stamp.draw(c.ink.a.f, c.ink.w, c.ink.h, 1, 1, (half ? 0.5 : 1) * grid.pitch * 14);
  };

  const frame = (c: Scene) => {
    const k = s.dt * 60;
    current.advance(s.dt, s.t);
    uploadField(c);
    // the ink
    const hold = Math.min(1, Math.max(0, (s.sincePour - HOLD_SECONDS) / 1.5));
    uniforms(gl, c.inkStep, {
      u_res: [W, H],
      u_mouse: s.mouse,
      u_mvel: s.mvel,
      u_decay: Math.pow(INK_DECAY, k),
      u_adv: INK_ADV * k,
      u_stir: INK_STIR,
      u_hold: hold,
    });
    bindField(c, c.inkStep);
    bindTex(gl, c.inkStep, 'u_prev', c.ink.a.t, 0);
    drawTriangle(gl, c.inkStep, c.ink.b.f, c.ink.w, c.ink.h);
    [c.ink.a, c.ink.b] = [c.ink.b, c.ink.a];
    // the motes and their trails
    motes.update(s.dt, current, MOTE_SPEED * k, s.mouse, s.moved, aspect);
    uniforms(gl, c.fade, { u_fade: Math.pow(TRAIL_FADE, k) });
    bindTex(gl, c.fade, 'u_prev', c.trails.a.t, 0);
    drawTriangle(gl, c.fade, c.trails.b.f, c.trails.w, c.trails.h);
    c.motePoints.pos.set(motes.pos);
    c.motePoints.col.set(motes.col);
    c.motePoints.alp.set(motes.alp);
    c.motePoints.draw(c.trails.b.f, c.trails.w, c.trails.h, half ? 0.085 : 0.11);
    [c.trails.a, c.trails.b] = [c.trails.b, c.trails.a];
    // the night as shown
    bindTex(gl, c.show, 'u_ink', c.ink.a.t, 0);
    bindTex(gl, c.show, 'u_trails', c.trails.a.t, 1);
    drawTriangle(gl, c.show, null, W, H);
    s.mvel = [s.mvel[0] * 0.8, s.mvel[1] * 0.8];
  };

  // the pointer: it stirs the ink and scatters the motes
  let last: [number, number] = [0.5, 0.5];
  const onMove = (e: PointerEvent) => {
    const r = canvas.getBoundingClientRect();
    if (!(r.width > 0) || !(r.height > 0)) return;
    const m: [number, number] = [
      (e.clientX - r.left) / r.width,
      1 - (e.clientY - r.top) / r.height,
    ];
    s.mvel = [m[0] - last[0], m[1] - last[1]];
    last = m;
    s.mouse = m;
    s.moved = true;
  };
  scene.addEventListener('pointermove', onMove);

  // the context's life. `stop()` frees what was built but leaves the context with its canvas, so a
  // remount (React's strict effects, a resize) builds on a live one; a context the night gave back
  // while off screen is asked for again through the handle kept per canvas.
  const lose = gl.getExtension('WEBGL_lose_context') ?? loseExtensions.get(canvas) ?? null;
  if (lose) loseExtensions.set(canvas, lose);
  const destroy = () => {
    if (!sc) return;
    for (const t of [sc.ink.a, sc.ink.b, sc.trails.a, sc.trails.b]) deleteTarget(gl, t);
    gl.deleteTexture(sc.fieldA);
    gl.deleteTexture(sc.fieldB);
    gl.deleteProgram(sc.inkStep);
    gl.deleteProgram(sc.fade);
    gl.deleteProgram(sc.show);
    sc.stamp.dispose();
    sc.motePoints.dispose();
    sc = null;
  };

  if (opts.still) {
    if (gl.isContextLost()) {
      scene.removeEventListener('pointermove', onMove);
      report('failed');
      return stopped();
    }
    try {
      sc = build();
      s.dt = 0;
      frame(sc);
      report('still');
    } catch (error) {
      console.error('night:', error);
      report('failed');
    }
    return {
      pour() {},
      stop() {
        scene.removeEventListener('pointermove', onMove);
        destroy();
      },
    };
  }

  // the loop and the view
  let raf = 0;
  let lost = gl.isContextLost();
  let visible = true;
  let released = false;
  let offTimer: ReturnType<typeof setTimeout> | null = null;
  const loop = () => {
    raf = 0;
    if (lost || !visible || !sc) return;
    const now = performance.now();
    s.dt = Math.min(0.05, (now - lastT) / 1000);
    lastT = now;
    s.t = (now - t0) / 1000;
    s.sincePour += s.dt;
    try {
      frame(sc);
    } catch (error) {
      console.error('night:', error);
      report('failed');
      return;
    }
    raf = requestAnimationFrame(loop);
  };
  const resume = () => {
    if (!raf && !lost && visible && sc) {
      lastT = performance.now();
      raf = requestAnimationFrame(loop);
    }
  };
  const onLost = (e: Event) => {
    e.preventDefault();
    lost = true;
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    sc = null; // everything built went with the context
    report('lost');
  };
  const onRestored = () => {
    try {
      sc = build();
      lost = false;
      released = false;
      report('running');
      resume();
    } catch (error) {
      console.error('night:', error);
      report('failed');
    }
  };
  canvas.addEventListener('webglcontextlost', onLost);
  canvas.addEventListener('webglcontextrestored', onRestored);
  const io =
    typeof IntersectionObserver === 'function'
      ? new IntersectionObserver((entries) => {
          for (const entry of entries) {
            visible = entry.isIntersecting;
            if (visible) {
              if (offTimer) {
                clearTimeout(offTimer);
                offTimer = null;
              }
              if (released && lose) lose.restoreContext();
              else resume();
            } else if (!offTimer) {
              offTimer = setTimeout(() => {
                offTimer = null;
                if (lose && !lost && !released) {
                  released = true;
                  lose.loseContext();
                }
              }, RELEASE_AFTER_MS);
            }
          }
        })
      : null;
  io?.observe(canvas);
  const teardown = () => {
    if (raf) cancelAnimationFrame(raf);
    if (offTimer) clearTimeout(offTimer);
    io?.disconnect();
    canvas.removeEventListener('webglcontextlost', onLost);
    canvas.removeEventListener('webglcontextrestored', onRestored);
    scene.removeEventListener('pointermove', onMove);
  };

  if (lost) {
    report('lost');
    if (lose) lose.restoreContext();
    else report('failed');
  } else {
    try {
      sc = build();
    } catch (error) {
      teardown();
      console.error('night:', error);
      report('failed');
      return stopped();
    }
    report('running');
    loop();
  }

  return {
    pour(next) {
      const bits = bitsOf(next);
      hash = next;
      current.retarget(seedOf(next));
      s.sincePour = 0;
      if (sc && !lost) {
        stampGrid(sc, bits);
        motes.spawn(bits, grid, W, H);
      }
    },
    stop() {
      teardown();
      if (!lost) destroy();
      else sc = null;
    },
  };
}
