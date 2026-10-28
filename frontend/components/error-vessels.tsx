'use client';

import { useEffect, useRef, useState, type RefObject } from 'react';
import { useErrorVessels, useVesselCaption } from '@/hooks/useErrorVessels';
import { layoutVessels, readVessel, type DrawVessel, type VesselRect } from '@/lib/error-vessels';
import type { VesselSnapshot } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import Link from '@/components/ui/link';

interface Props {
  className?: string;
  avoidRef?: RefObject<HTMLElement | null>;
}

const LAYERS = [
  'rgba(222,226,236,0.58)',
  'rgba(122,167,214,0.74)',
  'rgba(181,199,122,0.76)',
  'rgba(111,195,217,0.80)',
];

function drawCell(
  ctx: CanvasRenderingContext2D,
  vessel: DrawVessel,
  rect: VesselRect,
  opacity: number,
  fillProgress: number,
  avoid: DOMRect | null
) {
  const x = rect.x + 2;
  const y = rect.y + 2;
  const w = rect.width - 4;
  const h = rect.height - 4;
  if (w < 3 || h < 3) return;
  const centerX = x + w / 2;
  const centerY = y + h / 2;
  const covered =
    avoid &&
    centerX >= avoid.left &&
    centerX <= avoid.right &&
    centerY >= avoid.top &&
    centerY <= avoid.bottom;
  ctx.globalAlpha = opacity * (covered ? 0.12 : 1);
  ctx.fillStyle = 'rgba(222,226,236,0.025)';
  ctx.strokeStyle = 'rgba(222,226,236,0.33)';
  ctx.lineWidth = 0.8;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 2);
  ctx.fill();
  ctx.stroke();
  const layers = vessel.cell.anatomy;
  const bytes = [
    layers.capacityFieldBytes,
    layers.lockScriptBytes,
    layers.typeScriptBytes,
    layers.dataBytes,
  ];
  const usableHeight = Math.max(0, h - 2) * vessel.fullness * fillProgress;
  let bottom = y + h - 1;
  bytes.forEach((size, index) => {
    if (size === 0) return;
    const layerHeight = (usableHeight * size) / layers.totalBytes;
    ctx.fillStyle = LAYERS[index];
    ctx.fillRect(x + 1, bottom - layerHeight, Math.max(0, w - 2), layerHeight);
    bottom -= layerHeight;
  });
  ctx.globalAlpha = 1;
}

export function ErrorVessels({ className, avoidRef }: Props) {
  const network = useActiveNetwork();
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const previousRef = useRef<VesselSnapshot | null>(null);
  const [active, setActive] = useState(true);
  const { snapshot, change } = useErrorVessels(active);

  useEffect(() => {
    previousRef.current = null;
  }, [network]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const refresh = () => setActive(!document.hidden);
    document.addEventListener('visibilitychange', refresh);
    let observer: IntersectionObserver | null = null;
    if ('IntersectionObserver' in window) {
      observer = new IntersectionObserver((entries) =>
        setActive(!document.hidden && entries[0].isIntersecting)
      );
      observer.observe(host);
    }
    return () => {
      document.removeEventListener('visibilitychange', refresh);
      observer?.disconnect();
    };
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    const canvas = canvasRef.current;
    if (!host || !canvas) return;
    if (!snapshot) {
      canvas.getContext('2d')?.clearRect(0, 0, canvas.width, canvas.height);
      return;
    }
    const context = canvas.getContext('2d');
    if (!context) return;
    let frame = 0;
    let cancelled = false;
    const reducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
    const previous = previousRef.current;
    const current = snapshot.cells.map((cell) => readVessel(cell, previous?.anchorBlock ?? null));
    const departed = (change?.consumed ?? []).flatMap((outpoint) => {
      const cell = previous?.cells.find((candidate) => candidate.outpoint === outpoint);
      return cell ? [readVessel(cell, null)] : [];
    });
    const entered = new Set(
      change?.entered
        .filter((cell) => cell.entrance === 'created')
        .map((cell) => cell.cell.outpoint)
    );
    const render = (now: number, start: number) => {
      if (cancelled) return;
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      const width = host.clientWidth;
      const height = host.clientHeight;
      if (width <= 0 || height <= 0) return;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
      context.clearRect(0, 0, width, height);
      const box = host.getBoundingClientRect();
      const avoidBox = avoidRef?.current?.getBoundingClientRect();
      const avoid = avoidBox
        ? new DOMRect(
            avoidBox.left - box.left,
            avoidBox.top - box.top,
            avoidBox.width,
            avoidBox.height
          )
        : null;
      const rects = layoutVessels(current, width, height);
      const oldRects = previous
        ? layoutVessels(
            previous.cells.map((cell) => readVessel(cell, null)),
            width,
            height
          )
        : new Map<string, VesselRect>();
      const progress = reducedMotion ? 1 : Math.min(1, (now - start) / 1200);
      for (const vessel of current) {
        const target = rects.get(vessel.cell.outpoint);
        if (!target) continue;
        const old = oldRects.get(vessel.cell.outpoint);
        const rect =
          old && !reducedMotion
            ? {
                x: old.x + (target.x - old.x) * progress,
                y: old.y + (target.y - old.y) * progress,
                width: old.width + (target.width - old.width) * progress,
                height: old.height + (target.height - old.height) * progress,
              }
            : target;
        drawCell(context, vessel, rect, 1, entered.has(vessel.cell.outpoint) ? progress : 1, avoid);
      }
      for (const vessel of departed) {
        const old = oldRects.get(vessel.cell.outpoint);
        if (old && progress < 1) drawCell(context, vessel, old, 1 - progress, 1 - progress, avoid);
      }
      if (!reducedMotion && progress < 1 && active)
        frame = requestAnimationFrame((time) => render(time, start));
    };
    frame = requestAnimationFrame((time) => render(time, time));
    previousRef.current = snapshot;
    const observer =
      'ResizeObserver' in window
        ? new ResizeObserver(() => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame((time) => render(time, time - 1200));
          })
        : null;
    observer?.observe(host);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      observer?.disconnect();
    };
  }, [active, avoidRef, change, snapshot]);

  return (
    <div
      ref={hostRef}
      className={className ?? 'pointer-events-none absolute inset-0'}
      aria-hidden="true"
    >
      <canvas key={network} ref={canvasRef} className="h-full w-full" />
    </div>
  );
}

export function VesselsCaption() {
  const network = useActiveNetwork();
  const {
    network: sampleNetwork,
    state,
    anchorBlock,
    visible,
    blocksInWindow,
    message,
  } = useVesselCaption();
  if (sampleNetwork !== network || state === 'loading')
    return <span>Reading a bounded sample of live Common knowledge cells…</span>;
  if (state === 'error') return <span>Chain cell sample unavailable: {message}</span>;
  if (anchorBlock === null) return <span>Chain cell artwork paused.</span>;
  return (
    <span>
      {visible} live Common knowledge cells from {network}: first 6 transactions and first 4 outputs
      in each of {blocksInWindow} recent indexed blocks, up to 64 vessels, anchored at{' '}
      <Link href={`/blocks/${anchorBlock}`} className="underline">
        block #{anchorBlock.toLocaleString()}
      </Link>
      .{' '}
      {state === 'live'
        ? 'Following new blocks.'
        : state === 'waiting'
          ? 'Waiting for the indexed block.'
          : state === 'stale'
            ? 'Disconnected; sample may be stale.'
            : 'Artwork paused.'}{' '}
      Each vessel shows exact occupied bytes within its capacity; the diagram area is illustrative.
    </span>
  );
}
