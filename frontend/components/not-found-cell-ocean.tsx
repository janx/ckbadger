'use client';

import { useEffect, useRef, useState } from 'react';

const CELL_PX = 28;
const STEP_EVERY = 50;

/* Vertex */
const VERT = `
attribute vec2 a;
void main(){gl_Position=vec4(a,0,1);}
`;

/* Simulation — GoL B3/S23 + sparse spawn
   r = alive, g = phase (fade in/out), b = birth offset (set once, frozen) */
const SIM = `
precision highp float;
uniform sampler2D u_s;
uniform vec2 u_t;
uniform float u_step,u_seed;
float h(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
void main(){
  vec2 uv=gl_FragCoord.xy*u_t;
  vec4 s=texture2D(u_s,uv);
  float a=step(.5,s.r),ph=s.g,off=s.b,nx=a;
  if(u_step>.5){
    float n=0.;
    for(int y=-1;y<=1;y++)for(int x=-1;x<=1;x++){
      if(x==0&&y==0)continue;
      n+=step(.5,texture2D(u_s,uv+vec2(float(x),float(y))*u_t).r);
    }
    nx=a>.5?(n>1.5&&n<3.5?1.:0.):(n>2.5&&n<3.5?1.:0.);
    float w1=sin(uv.x*6.28+uv.y*4.2+u_seed*.03);
    float w2=sin(uv.x*3.5-uv.y*5.8+u_seed*.02+2.);
    float sp=max(smoothstep(.98,1.,w1)*.07,smoothstep(.985,1.,w2)*.05);
    float r=h(gl_FragCoord.xy*.37+u_seed);
    float inj=max(step(1.-sp,r),step(.9999,r));
    nx=max(nx,inj);
  }
  ph=nx>.5?min(1.,ph+.020):max(0.,ph-.002);
  // set birth offset once
  if(a<.5&&nx>.5) off=h(gl_FragCoord.xy*.73+u_seed*1.1);
  gl_FragColor=vec4(nx,ph,off,1);
}
`;

/* Render — innate identity diversity + breathing + bloom
   Each cell's personality is determined by its grid position (hash),
   NOT by age or life stage. Diversity is who you ARE, not how old you are.
   h1 = color identity, h2 = size/bloom character, h3 = breathing rhythm */
const RENDER = `
precision highp float;
uniform sampler2D u_s;
uniform vec2 u_r,u_g;
uniform float u_time;

float h(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}

void main(){
  vec2 uv=gl_FragCoord.xy/u_r;
  vec2 cell=uv*u_g, id=floor(cell), fr=fract(cell), tx=1./u_g;

  vec3 col=vec3(.018,.020,.038);

  // color palette endpoints
  vec3 jade=vec3(.18,.86,.64);
  vec3 aqua=vec3(.35,.75,.95);
  vec3 gold=vec3(.92,.72,.28);
  vec3 rose=vec3(.85,.40,.55);
  vec3 lilac=vec3(.60,.45,.85);

  // 5x5 neighborhood for wide bloom
  for(int iy=-2;iy<=2;iy++){
    for(int ix=-2;ix<=2;ix++){
      vec2 nId=id+vec2(float(ix),float(iy));
      vec4 st=texture2D(u_s,(nId+.5)*tx);
      float ph=st.g, off=st.b;
      if(ph<.003)continue;

      // --- Three independent identity hashes (innate, permanent) ---
      float h1=fract(sin(dot(nId,vec2(13.7,7.3)))*437.5);   // color
      float h2=fract(sin(dot(nId,vec2(91.3,47.1)))*317.8);   // size & bloom
      float h3=fract(sin(dot(nId,vec2(53.9,29.4)))*641.2);   // rhythm

      // --- Color identity: 5-stop gradient, each cell picks its hue ---
      vec3 cellCol;
      float ci=h1*4.;
      if(ci<1.) cellCol=mix(jade,aqua,ci);
      else if(ci<2.) cellCol=mix(aqua,gold,ci-1.);
      else if(ci<3.) cellCol=mix(gold,rose,ci-2.);
      else cellCol=mix(rose,lilac,ci-3.);

      // --- Breathing: rhythm is personality, not life stage ---
      float bSpeed=mix(.20,.55,h3);           // some naturally slow, some quick
      float bDepth=mix(.08,.30,1.-h3);        // slow breathers go deeper
      float breath=sin(u_time*bSpeed+off*6.28)*.5+.5;
      float br=ph*mix(1.-bDepth,1.,breath);

      // smooth fade on death (no binary jump)
      float fade=smoothstep(0.,.4,ph);

      vec2 lp=fr-vec2(float(ix)+.5,float(iy)+.5);
      float d=length(lp);
      float cheby=max(abs(lp.x),abs(lp.y));

      // --- Size identity: some cells naturally large, some small (square) ---
      float coreSz=mix(.24,.50,h2);
      float fill=smoothstep(coreSz+.04,coreSz-.10,cheby);

      // --- Bloom identity: small-core cells → wide soft glow (star),
      //     large-core cells → tight focused glow (ember) ---
      float bw1=mix(.35,1.2,h2);     // small→wide atmosphere, large→tight
      float bw2=mix(1.5,4.0,h2);     // same logic for mid halo
      float ba1=mix(.35,.18,h2);      // small→strong wash, large→subtle
      float bloom=exp(-d*d*bw1)*ba1
                 +exp(-d*d*bw2)*.16
                 +exp(-d*d*10.)*.06;

      // --- Subtle inner shimmer at cell's own tempo ---
      float shim=sin(d*8.+u_time*mix(.3,.6,h1)+h1*6.28)*.5+.5;
      vec3 pc=mix(cellCol,cellCol*1.2,shim*.15);
      pc=mix(pc*vec3(.4,.45,.5),pc,fade);  // desaturate on death
      float pi=mix(.12,.42,fade);

      vec3 bc=mix(cellCol*.6,pc,.3);

      col+=pc*fill*pi*br*.6;   // core
      col+=bc*bloom*br;        // atmosphere
    }
  }

  // --- Horizontal tear ---
  vec2 c=(gl_FragCoord.xy-u_r*.5)/u_r.y;
  float ty=mod(u_time*.15,1.)*2.-1.;
  float tear=smoothstep(.015,0.,abs(c.y-ty))*step(fract(u_time*.27),.12);
  col.r+=tear*.15; col.b-=tear*.06;

  // --- Film grain ---
  col+=(h(gl_FragCoord.xy+fract(u_time*37.)*773.)-.5)*.012;

  // --- Vignette (deep) ---
  float vig=1.-dot(c,c)*2.0;
  col*=smoothstep(0.,.45,vig);

  // --- Chromatic aberration ---
  float ed=length(c);
  col.r*=1.+smoothstep(.3,.55,ed)*.03;
  col.b*=1.-smoothstep(.3,.55,ed)*.02;

  gl_FragColor=vec4(clamp(col,0.,1.),1.);
}
`;

/* --- WebGL helpers --- */

function cc(gl: WebGLRenderingContext, type: number, src: string) {
  const s = gl.createShader(type);
  if (!s) throw new Error('shader');
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
    const e = gl.getShaderInfoLog(s);
    gl.deleteShader(s);
    throw new Error(`shader: ${e}`);
  }
  return s;
}

function lp(gl: WebGLRenderingContext, vs: string, fs: string) {
  const v = cc(gl, gl.VERTEX_SHADER, vs),
    f = cc(gl, gl.FRAGMENT_SHADER, fs);
  const p = gl.createProgram();
  if (!p) {
    gl.deleteShader(v);
    gl.deleteShader(f);
    throw new Error('prog');
  }
  gl.attachShader(p, v);
  gl.attachShader(p, f);
  gl.bindAttribLocation(p, 0, 'a');
  gl.linkProgram(p);
  gl.deleteShader(v);
  gl.deleteShader(f);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
    const e = gl.getProgramInfoLog(p);
    gl.deleteProgram(p);
    throw new Error(`link: ${e}`);
  }
  return p;
}

function tex(gl: WebGLRenderingContext, w: number, h: number, d: Uint8Array | null) {
  const t = gl.createTexture();
  if (!t) throw new Error('tex');
  gl.bindTexture(gl.TEXTURE_2D, t);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, d);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return t;
}

function seed(w: number, h: number) {
  const b = new Uint8Array(w * h * 4);
  for (let i = 0; i < w * h; i++) {
    const a = Math.random() < 0.045 ? 255 : 0,
      o = i * 4;
    b[o] = a;
    b[o + 1] = a ? 230 : 0; // high initial phase
    b[o + 2] = a ? Math.floor(Math.random() * 255) : 0; // breathing offset
    b[o + 3] = 255;
  }
  return b;
}

/* --- Component --- */

export function NotFoundCellOcean() {
  const ref = useRef<HTMLCanvasElement | null>(null);
  const [fb, setFb] = useState(false);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    let gl: WebGLRenderingContext | null = null;
    try {
      gl = cv.getContext('webgl', {
        antialias: false,
        alpha: false,
        depth: false,
        stencil: false,
        powerPreference: 'high-performance',
      });
    } catch {
      setFb(true);
      return;
    }
    if (!gl) {
      setFb(true);
      return;
    }

    const gW = Math.min(320, Math.max(32, Math.ceil(cv.clientWidth / CELL_PX)));
    const gH = Math.min(320, Math.max(18, Math.ceil(cv.clientHeight / CELL_PX)));

    let sp: WebGLProgram, rp: WebGLProgram;
    try {
      sp = lp(gl, VERT, SIM);
      rp = lp(gl, VERT, RENDER);
    } catch (e) {
      console.error('GoL init:', e);
      setFb(true);
      return;
    }

    const qb = gl.createBuffer();
    if (!qb) {
      setFb(true);
      return;
    }
    gl.bindBuffer(gl.ARRAY_BUFFER, qb);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW
    );
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

    const txs = [tex(gl, gW, gH, seed(gW, gH)), tex(gl, gW, gH, seed(gW, gH))];
    const f0 = gl.createFramebuffer(),
      f1 = gl.createFramebuffer();
    if (!f0 || !f1) {
      setFb(true);
      return;
    }
    const fbs = [f0, f1];
    for (let i = 0; i < 2; i++) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbs[i]);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, txs[i], 0);
      if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
        setFb(true);
        return;
      }
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    const su = {
      s: gl.getUniformLocation(sp, 'u_s'),
      t: gl.getUniformLocation(sp, 'u_t'),
      step: gl.getUniformLocation(sp, 'u_step'),
      seed: gl.getUniformLocation(sp, 'u_seed'),
    };
    const ru = {
      s: gl.getUniformLocation(rp, 'u_s'),
      r: gl.getUniformLocation(rp, 'u_r'),
      g: gl.getUniformLocation(rp, 'u_g'),
      time: gl.getUniformLocation(rp, 'u_time'),
    };

    const resize = () => {
      const d = Math.min(window.devicePixelRatio || 1, 2);
      const w = Math.max(1, Math.floor(cv.clientWidth * d));
      const h = Math.max(1, Math.floor(cv.clientHeight * d));
      if (cv.width !== w || cv.height !== h) {
        cv.width = w;
        cv.height = h;
      }
    };
    resize();
    window.addEventListener('resize', resize);

    let ri = 0,
      fc = 0,
      af = 0;
    const t0 = performance.now();
    const frame = (now: number) => {
      const el = (now - t0) / 1000,
        ds = fc % STEP_EVERY === 0 ? 1.0 : 0.0;
      fc++;
      const wi = 1 - ri;
      gl!.bindFramebuffer(gl!.FRAMEBUFFER, fbs[wi]);
      gl!.viewport(0, 0, gW, gH);
      gl!.useProgram(sp);
      gl!.activeTexture(gl!.TEXTURE0);
      gl!.bindTexture(gl!.TEXTURE_2D, txs[ri]);
      gl!.uniform1i(su.s, 0);
      gl!.uniform2f(su.t!, 1 / gW, 1 / gH);
      gl!.uniform1f(su.step!, ds);
      gl!.uniform1f(su.seed!, el * 7.3);
      gl!.drawArrays(gl!.TRIANGLES, 0, 6);
      ri = wi;

      gl!.bindFramebuffer(gl!.FRAMEBUFFER, null);
      gl!.viewport(0, 0, cv.width, cv.height);
      gl!.useProgram(rp);
      gl!.activeTexture(gl!.TEXTURE0);
      gl!.bindTexture(gl!.TEXTURE_2D, txs[ri]);
      gl!.uniform1i(ru.s, 0);
      gl!.uniform2f(ru.r!, cv.width, cv.height);
      gl!.uniform2f(ru.g!, gW, gH);
      gl!.uniform1f(ru.time!, el);
      gl!.drawArrays(gl!.TRIANGLES, 0, 6);
      af = requestAnimationFrame(frame);
    };
    af = requestAnimationFrame(frame);

    return () => {
      window.removeEventListener('resize', resize);
      cancelAnimationFrame(af);
      gl!.deleteBuffer(qb);
      txs.forEach((t) => gl!.deleteTexture(t));
      fbs.forEach((f) => gl!.deleteFramebuffer(f));
      gl!.deleteProgram(sp);
      gl!.deleteProgram(rp);
    };
  }, []);

  return (
    <div className="absolute inset-0">
      <canvas ref={ref} className="h-full w-full" aria-hidden="true" />
      {fb && (
        <div
          className="absolute inset-0 animate-pulse bg-[radial-gradient(circle_at_20%_20%,rgba(46,219,163,0.15),transparent_35%),radial-gradient(circle_at_80%_70%,rgba(104,204,240,0.10),transparent_40%),radial-gradient(circle_at_52%_45%,rgba(46,219,163,0.12),transparent_30%)]"
          aria-hidden="true"
        />
      )}
      <div
        className="absolute inset-0 bg-[radial-gradient(circle_at_center,transparent_0%,rgba(2,6,23,0.45)_68%,rgba(2,6,23,0.85)_100%)]"
        aria-hidden="true"
      />
    </div>
  );
}
