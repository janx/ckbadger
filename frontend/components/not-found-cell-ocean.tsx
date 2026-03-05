'use client';

import { useEffect, useRef, useState } from 'react';

interface NotFoundCellOceanProps {
  cellCount: number;
  splitPulse: number;
  haloBloom: number;
  motionSpeed: number;
}

const VERTEX_SHADER_SOURCE = `
attribute vec2 a_position;

void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

const FRAGMENT_SHADER_SOURCE = `
precision highp float;

uniform vec2 u_resolution;
uniform float u_time;
uniform float u_cell_count;
uniform float u_split_pulse;
uniform float u_halo_bloom;
uniform float u_motion_speed;

float hash(float n) {
  return fract(sin(n) * 43758.5453123);
}

vec2 swimmer(float seed, float t) {
  float speed = 0.14 + hash(seed * 3.17) * 0.28;
  float drift = 0.22 + hash(seed * 7.31) * 0.45;
  float phase = hash(seed * 11.7) * 6.28318530718;
  return vec2(
    0.5 + 0.35 * sin(t * speed + phase) + 0.08 * sin(t * (speed + drift) + phase * 1.7),
    0.5 + 0.35 * cos(t * (speed * 1.23) + phase * 1.31) + 0.08 * cos(t * (speed + drift * 0.77))
  );
}

void main() {
  float t = u_time * u_motion_speed;
  vec2 uv = gl_FragCoord.xy / u_resolution.xy;
  float aspect = u_resolution.x / u_resolution.y;
  uv.x = (uv.x - 0.5) * aspect + 0.5;

  float field = 0.0;
  float haloField = 0.0;

  const int MAX_CELL_COUNT = 36;
  for (int i = 0; i < MAX_CELL_COUNT; i++) {
    float fi = float(i);
    if (fi >= u_cell_count) {
      continue;
    }
    float seed = fi + 1.0;
    vec2 center = swimmer(seed, t);

    float split = 0.5 + 0.5 * sin(t * (0.45 + hash(seed * 2.1) * 0.7) + seed * 2.7);
    float angle = t * (0.2 + hash(seed * 5.3) * 0.6) + seed * 1.91;
    vec2 dir = vec2(cos(angle), sin(angle));
    float splitDistance = mix(0.003, 0.02 + 0.03 * u_split_pulse, split * split);

    float radius = mix(0.012, 0.028, hash(seed * 13.7));
    float pulse = 0.82 + (0.24 + 0.14 * u_split_pulse) * sin(t * (1.0 + hash(seed * 4.4) * 1.6) + seed * 4.0);
    float activeRadius = radius * pulse;

    vec2 p1 = center + dir * splitDistance;
    vec2 p2 = center - dir * splitDistance;

    float d1 = distance(uv, p1);
    float d2 = distance(uv, p2);

    field += activeRadius / (d1 * d1 + 0.0007);
    field += activeRadius / (d2 * d2 + 0.0007);

    haloField += activeRadius * (0.7 + u_halo_bloom) / (d1 * d1 + 0.0018);
    haloField += activeRadius * (0.7 + u_halo_bloom) / (d2 * d2 + 0.0018);
  }

  float core = smoothstep(2.2, 3.7, field);
  float body = smoothstep(1.0, 2.6, field);
  float halo = smoothstep(0.35, 1.5, haloField) - smoothstep(1.5, 3.0, haloField);

  vec3 bg = vec3(0.01, 0.03, 0.02);
  vec3 mid = vec3(0.02, 0.24, 0.10);
  vec3 bright = vec3(0.24, 1.00, 0.43);

  vec3 color = mix(bg, mid, body);
  color += bright * core * 0.9;
  color += vec3(0.16, 0.82, 0.34) * halo * (0.45 + 0.75 * u_halo_bloom);

  float vignette = smoothstep(0.95, 0.22, distance(uv, vec2(0.5)));
  color *= vignette;

  gl_FragColor = vec4(color, 1.0);
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

export function NotFoundCellOcean({
  cellCount,
  splitPulse,
  haloBloom,
  motionSpeed,
}: NotFoundCellOceanProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [useFallback, setUseFallback] = useState(false);
  const configRef = useRef({
    cellCount,
    splitPulse,
    haloBloom,
    motionSpeed,
  });

  useEffect(() => {
    configRef.current = {
      cellCount,
      splitPulse,
      haloBloom,
      motionSpeed,
    };
  }, [cellCount, splitPulse, haloBloom, motionSpeed]);

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
    const cellCountLoc = gl.getUniformLocation(program, 'u_cell_count');
    const splitPulseLoc = gl.getUniformLocation(program, 'u_split_pulse');
    const haloBloomLoc = gl.getUniformLocation(program, 'u_halo_bloom');
    const motionSpeedLoc = gl.getUniformLocation(program, 'u_motion_speed');
    if (
      !resolutionLoc ||
      !timeLoc ||
      !cellCountLoc ||
      !splitPulseLoc ||
      !haloBloomLoc ||
      !motionSpeedLoc
    ) {
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
      const config = configRef.current;
      gl.uniform2f(resolutionLoc, canvas.width, canvas.height);
      gl.uniform1f(timeLoc, elapsed);
      gl.uniform1f(cellCountLoc, config.cellCount);
      gl.uniform1f(splitPulseLoc, config.splitPulse);
      gl.uniform1f(haloBloomLoc, config.haloBloom);
      gl.uniform1f(motionSpeedLoc, config.motionSpeed);
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
