/**
 * The WebGL2 side of the night: the shaders and the few helpers around them. Everything that
 * touches a GL context lives here or in `night.ts`; the bits, the field and the motes are plain.
 */

export const VERTEX = `#version 300 es
precision highp float; const vec2 V[3]=vec2[3](vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.)); out vec2 v_uv;
void main(){ vec2 p=V[gl_VertexID]; v_uv=p*0.5+0.5; gl_Position=vec4(p,0.,1.); }`;

const JADE = 'vec3(0.18,0.86,0.64)';
const AQUA = 'vec3(0.41,0.80,0.94)';
const ROSE = 'vec3(1.0,0.52,0.69)';
const VIOL = 'vec3(0.66,0.43,0.80)';
const BASE = 'vec3(.030,.034,.058)';

/** The shared current: two fields blended, drifting and swaying — the same formula as `Current.sample`. */
const FIELD = `
uniform sampler2D u_fA; uniform sampler2D u_fB; uniform float u_blend; uniform float u_t;
vec2 field(vec2 uv){ vec2 q=uv+vec2(u_t*.0035,-u_t*.0015); vec2 v=mix(texture(u_fA,q).xy,texture(u_fB,q).xy,u_blend)*2.-1.;
  float s=sin(u_t*.11)*.3; return vec2(v.x*cos(s)-v.y*sin(s), v.x*sin(s)+v.y*cos(s)); }`;

/** The ink: dye advected along the current, decaying; the pointer stirs it. NaN never survives a frame. */
export const INK_STEP = `#version 300 es
precision highp float; in vec2 v_uv; out vec4 o; uniform sampler2D u_prev; uniform vec2 u_res; uniform vec2 u_mouse; uniform vec2 u_mvel; uniform float u_decay; uniform float u_adv; uniform float u_stir; uniform float u_hold; ${FIELD}
void main(){ vec2 uv=v_uv; vec2 ar=vec2(u_res.x/u_res.y,1.);
  vec2 v=field(uv)*u_adv*u_hold; vec2 d=(uv-u_mouse)*ar; float m=exp(-dot(d,d)*300.); v+=u_mvel*m*u_stir;
  vec3 c=texture(u_prev, uv-vec2(v.x/ar.x,v.y)).rgb*u_decay;
  if(any(isnan(c))) c=vec3(0.); o=vec4(min(c,vec3(1.)),1.); }`;

/** The trails fade; NaN never survives a frame. */
export const FADE = `#version 300 es
precision highp float; in vec2 v_uv; out vec4 o; uniform sampler2D u_prev; uniform float u_fade;
void main(){ vec3 c=texture(u_prev,v_uv).rgb*u_fade; if(any(isnan(c))) c=vec3(0.); o=vec4(c,1.); }`;

/** The night as shown: the theme's base, the ink, the trails, a vignette. */
export const SHOW = `#version 300 es
precision highp float; in vec2 v_uv; out vec4 o; uniform sampler2D u_ink; uniform sampler2D u_trails;
void main(){ float vig=1.-.45*pow(length(v_uv-.5)*1.1,2.2); vec3 col=${BASE};
  col+=texture(u_ink,v_uv).rgb*1.3; col+=texture(u_trails,v_uv).rgb;
  o=vec4(col*vig+${BASE}*(1.-vig),1.); }`;

export const POINT_VS = `#version 300 es
precision highp float; layout(location=0) in vec2 a_pos; layout(location=1) in float a_c; layout(location=2) in float a_a; uniform float u_size; out float v_c; out float v_a;
void main(){ v_c=a_c; v_a=a_a; gl_Position=vec4(a_pos*2.-1.,0.,1.); gl_PointSize=u_size; }`;

/** Points in the four hues: 0 jade, .5 aqua, 1 rose, 1.5 violet. */
export const POINT_FS = `#version 300 es
precision highp float; in float v_c; in float v_a; out vec4 o; uniform float u_gain;
void main(){ if(v_a<=0.) discard; vec3 c = v_c<.25 ? ${JADE} : (v_c<.75 ? ${AQUA} : (v_c<1.25 ? ${ROSE} : ${VIOL})); o=vec4(c*u_gain*v_a,1.); }`;

export type Format = 'u8' | 'f16' | 'f32';

export function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) throw new Error('night: createShader failed');
  gl.shaderSource(shader, src);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(`night: shader: ${gl.getShaderInfoLog(shader) ?? 'unknown error'}`);
  }
  return shader;
}

export function program(gl: WebGL2RenderingContext, fs: string, vs = VERTEX): WebGLProgram {
  const p = gl.createProgram();
  if (!p) throw new Error('night: createProgram failed');
  gl.attachShader(p, compile(gl, gl.VERTEX_SHADER, vs));
  gl.attachShader(p, compile(gl, gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
    throw new Error(`night: link: ${gl.getProgramInfoLog(p) ?? 'unknown error'}`);
  }
  return p;
}

export function texture(
  gl: WebGL2RenderingContext,
  w: number,
  h: number,
  fmt: Format,
  data: ArrayBufferView | null,
  wrap: number = gl.CLAMP_TO_EDGE
): WebGLTexture {
  const t = gl.createTexture();
  if (!t) throw new Error('night: createTexture failed');
  gl.bindTexture(gl.TEXTURE_2D, t);
  const internal = fmt === 'f32' ? gl.RGBA32F : fmt === 'f16' ? gl.RGBA16F : gl.RGBA8;
  const type = fmt === 'f32' ? gl.FLOAT : fmt === 'f16' ? gl.HALF_FLOAT : gl.UNSIGNED_BYTE;
  gl.texImage2D(gl.TEXTURE_2D, 0, internal, w, h, 0, gl.RGBA, type, data);
  const filter = fmt === 'f32' ? gl.NEAREST : gl.LINEAR;
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
  return t;
}

export interface Target {
  t: WebGLTexture;
  f: WebGLFramebuffer;
  w: number;
  h: number;
}

export function deleteTarget(gl: WebGL2RenderingContext, target: Target): void {
  gl.deleteFramebuffer(target.f);
  gl.deleteTexture(target.t);
}

/**
 * A render target. The feedback buffers are half-float: an 8-bit buffer cannot fade at a factor
 * near one (0.9975 rounds back to the same byte, and the ink never dissolves).
 */
export function target(gl: WebGL2RenderingContext, w: number, h: number, fmt: Format): Target {
  if (fmt !== 'u8' && !gl.getExtension('EXT_color_buffer_float')) {
    throw new Error('night: no float render targets');
  }
  const t = texture(gl, w, h, fmt, null);
  const f = gl.createFramebuffer();
  if (!f) throw new Error('night: createFramebuffer failed');
  gl.bindFramebuffer(gl.FRAMEBUFFER, f);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, t, 0);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  return { t, f, w, h };
}

export function uniforms(
  gl: WebGL2RenderingContext,
  p: WebGLProgram,
  map: Record<string, number | readonly number[]>
): void {
  gl.useProgram(p);
  for (const name of Object.keys(map)) {
    const value = map[name];
    const loc = gl.getUniformLocation(p, name);
    if (loc === null) continue;
    if (typeof value === 'number') gl.uniform1f(loc, value);
    else if (value.length === 2) gl.uniform2f(loc, value[0], value[1]);
    else if (value.length === 3) gl.uniform3f(loc, value[0], value[1], value[2]);
    else throw new Error(`night: uniform ${name} has ${value.length} components`);
  }
}

export function bindTex(
  gl: WebGL2RenderingContext,
  p: WebGLProgram,
  name: string,
  t: WebGLTexture,
  unit: number
): void {
  gl.useProgram(p);
  gl.activeTexture(gl.TEXTURE0 + unit);
  gl.bindTexture(gl.TEXTURE_2D, t);
  gl.uniform1i(gl.getUniformLocation(p, name), unit);
}

/** One full-screen triangle through program `p` into framebuffer `fb` (null = the canvas). */
export function drawTriangle(
  gl: WebGL2RenderingContext,
  p: WebGLProgram,
  fb: WebGLFramebuffer | null,
  w: number,
  h: number
): void {
  gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
  gl.viewport(0, 0, w, h);
  gl.useProgram(p);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
}

export interface Points {
  readonly N: number;
  readonly pos: Float32Array;
  readonly col: Float32Array;
  readonly alp: Float32Array;
  /** Additive points into `fb` (null = the canvas); `count` limits how many of the N are drawn. */
  dispose(): void;
  draw(
    fb: WebGLFramebuffer | null,
    w: number,
    h: number,
    gain: number,
    count?: number,
    size?: number
  ): void;
}

/** N points with a position, a hue index and an alpha each, drawn additively. */
export function points(gl: WebGL2RenderingContext, N: number, size: number): Points {
  const pos = new Float32Array(N * 2);
  const col = new Float32Array(N);
  const alp = new Float32Array(N);
  const p = program(gl, POINT_FS, POINT_VS);
  const vao = gl.createVertexArray();
  if (!vao) throw new Error('night: createVertexArray failed');
  gl.bindVertexArray(vao);
  const attach = (data: Float32Array, index: number, components: number) => {
    const buffer = gl.createBuffer();
    if (!buffer) throw new Error('night: createBuffer failed');
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(index);
    gl.vertexAttribPointer(index, components, gl.FLOAT, false, 0, 0);
    return buffer;
  };
  const bp = attach(pos, 0, 2);
  const bc = attach(col, 1, 1);
  const ba = attach(alp, 2, 1);
  gl.bindVertexArray(null);
  return {
    N,
    pos,
    col,
    alp,
    dispose() {
      gl.deleteBuffer(bp);
      gl.deleteBuffer(bc);
      gl.deleteBuffer(ba);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(p);
    },
    draw(fb, w, h, gain, count, sizeOverride) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      gl.viewport(0, 0, w, h);
      gl.useProgram(p);
      uniforms(gl, p, { u_size: sizeOverride ?? size, u_gain: gain });
      gl.bindVertexArray(vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, bp);
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, pos);
      gl.bindBuffer(gl.ARRAY_BUFFER, bc);
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, col);
      gl.bindBuffer(gl.ARRAY_BUFFER, ba);
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, alp);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.ONE, gl.ONE);
      gl.drawArrays(gl.POINTS, 0, count ?? N);
      gl.disable(gl.BLEND);
      gl.bindVertexArray(null);
    },
  };
}
