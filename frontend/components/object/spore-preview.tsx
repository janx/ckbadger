'use client';

import { useCallback, useMemo, useRef, useState } from 'react';
import { type PreviewKind } from '@/lib/preview-utils';

export type PreviewPhysicality = 'onchain' | 'onchain-btc' | 'default';

export interface SporePreviewProps {
  preview: PreviewKind;
  physicality?: PreviewPhysicality;
}

const MAX_TILT_DEG = 8;

const GLOW_COLORS: Record<PreviewPhysicality, string> = {
  onchain: 'rgba(64, 232, 176, 0.12)',
  'onchain-btc': 'rgba(200, 190, 100, 0.10)',
  default: 'transparent',
};

/**
 * Gallery-style artwork display for on-chain Spore content.
 *
 * For fully on-chain objects, the preview gains physicality:
 * - 3D tilt that follows the cursor (perspective + rotateX/Y)
 * - A light-reflection overlay that shifts with the tilt angle
 * - An ambient glow behind the artwork that breathes slowly
 * - Dynamic drop shadow that adjusts with tilt
 */
export function SporePreview({ preview, physicality = 'default' }: SporePreviewProps) {
  if (!preview) return null;

  const hasPhysics = physicality !== 'default';

  if (hasPhysics) {
    return <PhysicalPreview preview={preview} physicality={physicality} />;
  }

  return <StaticPreview preview={preview} />;
}

/* ---------- Static (non-onchain) ---------- */

function StaticPreview({ preview }: { preview: NonNullable<PreviewKind> }) {
  return (
    <div
      className="flex items-center justify-center rounded-md p-8"
      style={{
        background: '#070b10',
        boxShadow: 'inset 0 2px 8px rgba(0,0,0,0.7), inset 0 0 1px rgba(255,255,255,0.04)',
      }}
    >
      <div className="relative" style={{ filter: 'drop-shadow(0 4px 12px rgba(0,0,0,0.5))' }}>
        <PreviewContent preview={preview} />
      </div>
    </div>
  );
}

/* ---------- Physical (fully on-chain) ---------- */

function PhysicalPreview({
  preview,
  physicality,
}: {
  preview: NonNullable<PreviewKind>;
  physicality: PreviewPhysicality;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [tilt, setTilt] = useState({ rx: 0, ry: 0, active: false });

  const handleMouseMove = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const el = containerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    // Normalized -1 to 1 from center
    const nx = ((e.clientX - rect.left) / rect.width - 0.5) * 2;
    const ny = ((e.clientY - rect.top) / rect.height - 0.5) * 2;
    setTilt({ rx: -ny * MAX_TILT_DEG, ry: nx * MAX_TILT_DEG, active: true });
  }, []);

  const handleMouseLeave = useCallback(() => {
    setTilt({ rx: 0, ry: 0, active: false });
  }, []);

  const glowColor = GLOW_COLORS[physicality];

  // Light reflection position follows tilt (opposite direction for realism)
  const lightX = 50 - tilt.ry * 3; // percentage
  const lightY = 50 - tilt.rx * 3;

  // Shadow shifts opposite to tilt direction
  const shadowX = tilt.ry * 0.8;
  const shadowY = -tilt.rx * 0.8;
  const shadowBlur = tilt.active ? 20 : 12;

  return (
    <div
      className="flex items-center justify-center rounded-md p-8"
      style={{
        background: '#070b10',
        boxShadow: 'inset 0 2px 8px rgba(0,0,0,0.7), inset 0 0 1px rgba(255,255,255,0.04)',
      }}
    >
      {/* Ambient glow — breathes behind the artwork */}
      <div
        className="preview-ambient-glow absolute rounded-full"
        style={{
          width: '60%',
          height: '60%',
          background: `radial-gradient(ellipse at center, ${glowColor} 0%, transparent 70%)`,
          filter: 'blur(30px)',
          pointerEvents: 'none',
        }}
      />

      {/* 3D tilt container */}
      <div
        ref={containerRef}
        className="relative"
        style={{
          perspective: '800px',
          perspectiveOrigin: 'center center',
        }}
      >
        {/* Mouse tracking overlay — sits above iframe to capture events */}
        <div
          className="absolute inset-0 z-20"
          onMouseMove={handleMouseMove}
          onMouseLeave={handleMouseLeave}
        />
        <div
          style={{
            transform: `rotateX(${tilt.rx}deg) rotateY(${tilt.ry}deg)`,
            transition: tilt.active ? 'transform 0.08s ease-out' : 'transform 0.5s ease-out',
            transformStyle: 'preserve-3d',
            filter: `drop-shadow(${shadowX}px ${shadowY + 4}px ${shadowBlur}px rgba(0,0,0,0.5))`,
          }}
        >
          {/* Light reflection overlay */}
          <div
            className="pointer-events-none absolute inset-0 z-10 rounded"
            style={{
              background: `radial-gradient(ellipse at ${lightX}% ${lightY}%, rgba(255,255,255,0.08) 0%, transparent 60%)`,
              mixBlendMode: 'overlay',
            }}
          />
          <PreviewContent preview={preview} />
        </div>
      </div>
    </div>
  );
}

/* ---------- Shared content renderers ---------- */

function PreviewContent({ preview }: { preview: NonNullable<PreviewKind> }) {
  if (preview.type === 'image') {
    return <ImagePreview dataUrl={preview.dataUrl} />;
  }
  return <SvgPreview markup={preview.markup} />;
}

function ImagePreview({ dataUrl }: { dataUrl: string }) {
  return (
    <img
      src={dataUrl}
      alt="Spore content preview"
      className="max-h-80 max-w-full rounded object-contain"
      draggable={false}
    />
  );
}

function SvgPreview({ markup }: { markup: string }) {
  const srcDoc = useMemo(
    () =>
      `<!DOCTYPE html><html><head><style>*{margin:0;padding:0;box-sizing:border-box}html,body{height:100%;background:transparent;display:flex;align-items:center;justify-content:center;overflow:hidden}svg{display:block;max-width:100%;max-height:100%;height:100%}</style></head><body>${markup}</body></html>`,
    [markup]
  );

  return (
    <iframe
      sandbox=""
      srcDoc={srcDoc}
      title="Spore SVG preview"
      className="h-80 w-80 rounded border-0"
      style={{ colorScheme: 'normal' }}
    />
  );
}
