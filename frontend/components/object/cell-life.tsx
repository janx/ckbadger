'use client';

import { useEffect, useRef, useMemo, useCallback } from 'react';
import {
  hashToBytes,
  seedGrid,
  stepGrid,
  getShapeIndex,
  getInterval,
  SHAPE_DRAW_FNS,
} from '@/lib/game-of-life';

/* ---------- Colors ---------- */

const CKB_COLOR = { r: 46, g: 219, b: 163 };
const BTC_COLOR = { r: 242, g: 197, b: 92 };

const BG_CKB = '#0a0f12';
const BG_DUAL = '#0c0b08';

const GRID_LINE_CKB = 'rgba(46,219,163,0.05)';
const GRID_LINE_DUAL = 'rgba(180,170,120,0.04)';

const GLOW_CKB = 'rgba(46,219,163,0.08)';
const GLOW_DUAL = 'linear-gradient(135deg, rgba(242,197,92,0.06), rgba(46,219,163,0.06))';

/* ---------- CellLifePlaceholder ---------- */

interface CellLifePlaceholderProps {
  size?: number;
}

export function CellLifePlaceholder({ size = 56 }: CellLifePlaceholderProps) {
  return (
    <div
      style={{
        width: `${size}px`,
        height: `${size}px`,
        background: '#0f0f0f',
        border: '1px solid #222840',
        borderRadius: `${size * 0.14}px`,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: '#343c50',
        fontFamily: 'monospace',
        fontSize: `${size * 0.4}px`,
        lineHeight: 1,
        flexShrink: 0,
      }}
    >
      ?
    </div>
  );
}

/* ---------- CellLife ---------- */

interface CellLifeProps {
  hash: string;
  size?: number;
  gridSize?: number;
  isDualChain?: boolean;
}

/**
 * Conway's Game of Life visualization seeded by a hex hash.
 *
 * Renders onto a retina-aware canvas with a 4-layer bloom per alive cell.
 * Animation is driven by requestAnimationFrame, throttled by a hash-derived interval.
 */
export function CellLife({ hash, size = 56, gridSize = 8, isDualChain = false }: CellLifeProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef<number>(0);
  const pausedRef = useRef(false);

  const bytes = useMemo(() => hashToBytes(hash), [hash]);
  const shapeIndex = useMemo(() => getShapeIndex(bytes), [bytes]);
  const interval = useMemo(() => getInterval(bytes), [bytes]);

  const glowPhaseOffset = useMemo(() => {
    const b = bytes[0] ?? 0;
    return -((b % 40) / 10);
  }, [bytes]);

  const bg = isDualChain ? BG_DUAL : BG_CKB;
  const gridLineColor = isDualChain ? GRID_LINE_DUAL : GRID_LINE_CKB;
  const glowBg = isDualChain ? GLOW_DUAL : GLOW_CKB;

  const colorForValue = useCallback((val: number) => {
    if (val === 2) return BTC_COLOR;
    return CKB_COLOR;
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Retina support
    const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    const cellSize = size / gridSize;
    const drawShape = SHAPE_DRAW_FNS[shapeIndex];

    // State
    let grid = seedGrid(gridSize, bytes, isDualChain);
    let generation = 0;
    let lastTick = 0;

    // Opacity tracking: one per cell
    const opacities: number[][] = Array.from({ length: gridSize }, () => Array(gridSize).fill(0));
    // Last known color value per cell (for fading dead cells with correct color)
    const lastColor: number[][] = Array.from({ length: gridSize }, () => Array(gridSize).fill(1));
    // Initialize opacities from seed
    for (let r = 0; r < gridSize; r++) {
      for (let c = 0; c < gridSize; c++) {
        opacities[r][c] = grid[r][c] > 0 ? 1.0 : 0;
        if (grid[r][c] > 0) lastColor[r][c] = grid[r][c];
      }
    }

    // Check for prefers-reduced-motion
    const reducedMotion =
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    function renderFrame() {
      if (!ctx) return;
      // Clear
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, size, size);

      // Grid lines
      ctx.strokeStyle = gridLineColor;
      ctx.lineWidth = 0.5;
      for (let i = 1; i < gridSize; i++) {
        const pos = i * cellSize;
        ctx.beginPath();
        ctx.moveTo(pos, 0);
        ctx.lineTo(pos, size);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(0, pos);
        ctx.lineTo(size, pos);
        ctx.stroke();
      }

      // Draw cells
      for (let r = 0; r < gridSize; r++) {
        for (let c = 0; c < gridSize; c++) {
          const op = opacities[r][c];
          if (op <= 0) continue;

          const cx = c * cellSize + cellSize / 2;
          const cy = r * cellSize + cellSize / 2;
          const baseRadius = (cellSize / 2) * 0.85; // slight inset from cell edge
          const color = colorForValue(grid[r][c] > 0 ? grid[r][c] : lastColor[r][c]);

          const isAlive = grid[r][c] > 0;

          if (isAlive) {
            // Layer 1: outer bloom (1.8x, 8% opacity)
            ctx.globalAlpha = op * 0.08;
            ctx.fillStyle = `rgb(${color.r},${color.g},${color.b})`;
            drawShape(ctx, cx, cy, baseRadius * 1.8);

            // Layer 2: inner bloom (1.3x, 18% opacity)
            ctx.globalAlpha = op * 0.18;
            drawShape(ctx, cx, cy, baseRadius * 1.3);

            // Layer 3: cell body (1.0x, 75% opacity)
            ctx.globalAlpha = op * 0.75;
            drawShape(ctx, cx, cy, baseRadius);

            // Layer 4: core (0.45x, 90% opacity)
            ctx.globalAlpha = op * 0.9;
            drawShape(ctx, cx, cy, baseRadius * 0.45);
          } else {
            // Dead cell fading: 0.7x, 30% opacity * current opacity
            ctx.globalAlpha = op * 0.3;
            ctx.fillStyle = `rgb(${color.r},${color.g},${color.b})`;
            drawShape(ctx, cx, cy, baseRadius * 0.7);
          }
        }
      }

      ctx.globalAlpha = 1;
    }

    // Static render for reduced-motion or initial frame
    renderFrame();

    if (reducedMotion) {
      // Render gen 0 only, no animation
      return;
    }

    function tick() {
      // Step the grid
      const nextGrid = stepGrid(grid);
      generation++;

      // Update opacities
      for (let r = 0; r < gridSize; r++) {
        for (let c = 0; c < gridSize; c++) {
          const wasAlive = grid[r][c] > 0;
          const isAlive = nextGrid[r][c] > 0;

          if (isAlive && !wasAlive) {
            // Birth: jump opacity
            opacities[r][c] = Math.min(1, opacities[r][c] + 0.5);
            lastColor[r][c] = nextGrid[r][c];
          } else if (!isAlive && wasAlive) {
            // Death begins fading (lastColor preserved for fade-out)
          } else if (isAlive) {
            // Alive and staying alive: full opacity
            opacities[r][c] = Math.min(1, opacities[r][c] + 0.5);
            lastColor[r][c] = nextGrid[r][c];
          }
        }
      }

      grid = nextGrid;

      // Check for reset conditions
      let population = 0;
      for (let r = 0; r < gridSize; r++) {
        for (let c = 0; c < gridSize; c++) {
          if (grid[r][c] > 0) population++;
        }
      }

      if (population === 0 || generation > 250) {
        // Reseed
        grid = seedGrid(gridSize, bytes, isDualChain);
        generation = 0;
        for (let r = 0; r < gridSize; r++) {
          for (let c = 0; c < gridSize; c++) {
            opacities[r][c] = grid[r][c] > 0 ? 0.3 : 0;
            if (grid[r][c] > 0) lastColor[r][c] = grid[r][c];
          }
        }
      }
    }

    function animate(timestamp: number) {
      if (!pausedRef.current) {
        // Tick at the derived interval
        if (timestamp - lastTick >= interval) {
          // Decrease dead cell opacity by 0.12 per tick
          for (let r = 0; r < gridSize; r++) {
            for (let c = 0; c < gridSize; c++) {
              if (grid[r][c] === 0 && opacities[r][c] > 0) {
                opacities[r][c] = Math.max(0, opacities[r][c] - 0.12);
              }
            }
          }
          tick();
          lastTick = timestamp;
        }

        renderFrame();
      }

      rafRef.current = requestAnimationFrame(animate);
    }

    rafRef.current = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(rafRef.current);
    };
  }, [bytes, size, gridSize, isDualChain, shapeIndex, interval, bg, gridLineColor, colorForValue]);

  const handleMouseEnter = useCallback(() => {
    pausedRef.current = true;
  }, []);

  const handleMouseLeave = useCallback(() => {
    pausedRef.current = false;
  }, []);

  return (
    <div
      style={{
        position: 'relative',
        width: `${size}px`,
        height: `${size}px`,
        borderRadius: `${size * 0.14}px`,
        overflow: 'hidden',
      }}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      {/* Outer glow */}
      <div
        style={{
          position: 'absolute',
          inset: 0,
          borderRadius: 'inherit',
          background: glowBg,
          animation: 'glow-breathe 4s ease-in-out infinite',
          animationDelay: `${glowPhaseOffset}s`,
          pointerEvents: 'none',
        }}
      />
      <canvas
        ref={canvasRef}
        style={{
          width: `${size}px`,
          height: `${size}px`,
          display: 'block',
          borderRadius: 'inherit',
        }}
      />
    </div>
  );
}
