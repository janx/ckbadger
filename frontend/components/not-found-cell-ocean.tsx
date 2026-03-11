'use client';

import { useEffect, useRef, useState } from 'react';

const VERTEX_SHADER_SOURCE = `
attribute vec2 a_position;

void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

const FRAGMENT_SHADER_SOURCE = `
precision highp float;

uniform float u_time;
uniform vec2 u_resolution;

float hash2(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash2(i), hash2(i + vec2(1, 0)), f.x),
    mix(hash2(i + vec2(0, 1)), hash2(i + vec2(1, 1)), f.x),
    f.y
  );
}

float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  mat2 rot = mat2(0.8, 0.6, -0.6, 0.8);
  for (int i = 0; i < 6; i++) {
    v += a * vnoise(p);
    p = rot * p * 2.0;
    a *= 0.5;
  }
  return v;
}

void main() {
  vec2 uv = (gl_FragCoord.xy - u_resolution * 0.5) / u_resolution.y;

  // Occasional horizontal tear
  float tearY = mod(u_time * 0.15, 1.0) * 2.0 - 1.0;
  float tearStrength = smoothstep(0.015, 0.0, abs(uv.y - tearY))
                     * step(fract(u_time * 0.27), 0.12);
  uv.x += tearStrength * 0.04 * sin(u_time * 60.0);

  // Three noise scales
  float macro = fbm(uv * 6.0 + u_time * 0.06)
              + fbm(uv * 10.0 - u_time * 0.1 + 100.0) * 0.5;
  macro = smoothstep(0.48, 0.72, macro);

  float mid = fbm(uv * 14.0 + u_time * 0.18 + 50.0)
            + 0.25 * sin(u_time * 0.4 + fbm(uv * 7.0) * 6.28);
  mid = smoothstep(0.52, 0.78, mid);

  float fine = smoothstep(0.58, 0.82, fbm(uv * 24.0 + u_time * 0.35 + 200.0));

  float field = macro * 0.5 + macro * mid * 0.35 + macro * mid * fine * 0.15;

  // Petri dish clear zone
  field *= smoothstep(0.15, 0.45, length(uv));

  // Edge detection via finite-difference gradient
  float eps = 1.5 / u_resolution.y;
  float fx, fy;
  {
    vec2 u2 = uv + vec2(eps, 0);
    float c2 = smoothstep(0.15, 0.45, length(u2));
    float m2 = smoothstep(0.48, 0.72,
      fbm(u2 * 6.0 + u_time * 0.06) + fbm(u2 * 10.0 - u_time * 0.1 + 100.0) * 0.5);
    float i2 = smoothstep(0.52, 0.78,
      fbm(u2 * 14.0 + u_time * 0.18 + 50.0) + 0.25 * sin(u_time * 0.4 + fbm(u2 * 7.0) * 6.28));
    float f2 = smoothstep(0.58, 0.82, fbm(u2 * 24.0 + u_time * 0.35 + 200.0));
    fx = (m2 * 0.5 + m2 * i2 * 0.35 + m2 * i2 * f2 * 0.15) * c2;
  }
  {
    vec2 u2 = uv + vec2(0, eps);
    float c2 = smoothstep(0.15, 0.45, length(u2));
    float m2 = smoothstep(0.48, 0.72,
      fbm(u2 * 6.0 + u_time * 0.06) + fbm(u2 * 10.0 - u_time * 0.1 + 100.0) * 0.5);
    float i2 = smoothstep(0.52, 0.78,
      fbm(u2 * 14.0 + u_time * 0.18 + 50.0) + 0.25 * sin(u_time * 0.4 + fbm(u2 * 7.0) * 6.28));
    float f2 = smoothstep(0.58, 0.82, fbm(u2 * 24.0 + u_time * 0.35 + 200.0));
    fy = (m2 * 0.5 + m2 * i2 * 0.35 + m2 * i2 * f2 * 0.15) * c2;
  }
  float edge = smoothstep(0.0, 3.0, length(vec2(fx - field, fy - field)) / eps);

  // Color composition
  vec3 col = vec3(0.01, 0.015, 0.025);
  col = mix(col, vec3(0.04, 0.12, 0.09), field * 0.7);

  // Glowing cell membranes: jade #2edba3 / aqua #68ccf0 color mix
  col += mix(vec3(0.18, 0.86, 0.64), vec3(0.41, 0.80, 0.94),
    sin(u_time * 0.3 + uv.x * 3.0) * 0.5 + 0.5) * edge * field * 0.4;

  col += vec3(0.18, 0.86, 0.64) * smoothstep(0.7, 0.95, field) * 0.2;
  col += vec3(0.18, 0.86, 0.64) * smoothstep(0.0, 0.5, field) * 0.03;

  // Red death zones at cell boundary edges
  col += vec3(0.4, 0.1, 0.12)
       * smoothstep(0.02, 0.12, field)
       * (1.0 - smoothstep(0.12, 0.25, field))
       * 0.3;

  // Chromatic aberration from tear
  col.r += tearStrength * 0.35;
  col.b -= tearStrength * 0.15;

  // Film grain overlay
  col += (hash2(gl_FragCoord.xy + u_time * 0.1) - 0.5) * 0.02;

  gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}
`;

function compileShader(gl: WebGLRenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) {
    throw new Error('Unable to allocate WebGL shader.');
  }

  gl.shaderSource(shader, source);
  gl.compileShader(shader);

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const info = gl.getShaderInfoLog(shader) ?? 'unknown shader error';
    gl.deleteShader(shader);
    throw new Error(`Shader compile failure: ${info}`);
  }

  return shader;
}

function createProgram(gl: WebGLRenderingContext): WebGLProgram {
  const vertexShader = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER_SOURCE);
  const fragmentShader = compileShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER_SOURCE);

  const program = gl.createProgram();
  if (!program) {
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    throw new Error('Unable to allocate WebGL program.');
  }

  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);

  gl.deleteShader(vertexShader);
  gl.deleteShader(fragmentShader);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const info = gl.getProgramInfoLog(program) ?? 'unknown program link error';
    gl.deleteProgram(program);
    throw new Error(`Program link failure: ${info}`);
  }

  return program;
}

export function NotFoundCellOcean() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [useFallback, setUseFallback] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }

    let gl: WebGLRenderingContext | null = null;
    try {
      gl = canvas.getContext('webgl', {
        antialias: false,
        alpha: false,
        depth: false,
        stencil: false,
        powerPreference: 'high-performance',
      });
    } catch {
      setUseFallback(true);
      return;
    }

    if (!gl) {
      setUseFallback(true);
      return;
    }

    let program: WebGLProgram;
    let animationFrame = 0;

    try {
      program = createProgram(gl);
      gl.useProgram(program);
    } catch (error) {
      console.error('Failed to initialize 404 GLSL ocean:', error);
      setUseFallback(true);
      return;
    }

    const positionBuffer = gl.createBuffer();
    if (!positionBuffer) {
      console.error('Failed to allocate position buffer for 404 GLSL ocean.');
      setUseFallback(true);
      return;
    }

    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW
    );

    const positionLoc = gl.getAttribLocation(program, 'a_position');
    if (positionLoc < 0) {
      gl.deleteBuffer(positionBuffer);
      gl.deleteProgram(program);
      console.error('Missing a_position attribute in 404 GLSL program.');
      setUseFallback(true);
      return;
    }

    gl.enableVertexAttribArray(positionLoc);
    gl.vertexAttribPointer(positionLoc, 2, gl.FLOAT, false, 0, 0);

    const resolutionLoc = gl.getUniformLocation(program, 'u_resolution');
    const timeLoc = gl.getUniformLocation(program, 'u_time');
    if (!resolutionLoc || !timeLoc) {
      gl.deleteBuffer(positionBuffer);
      gl.deleteProgram(program);
      console.error('Missing uniforms in 404 GLSL program.');
      setUseFallback(true);
      return;
    }

    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const width = Math.max(1, Math.floor(canvas.clientWidth * dpr));
      const height = Math.max(1, Math.floor(canvas.clientHeight * dpr));

      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      gl.viewport(0, 0, width, height);
    };

    resize();
    window.addEventListener('resize', resize);

    const startedAt = performance.now();
    const renderFrame = (now: number) => {
      const elapsed = (now - startedAt) / 1000;
      gl.uniform2f(resolutionLoc, canvas.width, canvas.height);
      gl.uniform1f(timeLoc, elapsed);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
      animationFrame = window.requestAnimationFrame(renderFrame);
    };

    animationFrame = window.requestAnimationFrame(renderFrame);

    return () => {
      window.removeEventListener('resize', resize);
      window.cancelAnimationFrame(animationFrame);
      gl.deleteBuffer(positionBuffer);
      gl.deleteProgram(program);
    };
  }, []);

  return (
    <div className="absolute inset-0">
      <canvas ref={canvasRef} className="h-full w-full" aria-hidden="true" />
      {useFallback && (
        <div
          className="absolute inset-0 animate-pulse bg-[radial-gradient(circle_at_20%_20%,rgba(0,255,65,0.3),transparent_35%),radial-gradient(circle_at_80%_70%,rgba(0,204,51,0.24),transparent_40%),radial-gradient(circle_at_52%_45%,rgba(74,222,128,0.2),transparent_30%)]"
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
