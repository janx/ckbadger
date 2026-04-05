'use client';

import { useEffect, useRef } from 'react';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { NotFoundCellOcean } from '@/components/not-found-cell-ocean';
import { resolveBuildVersion } from '@/lib/runtime-config';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

const GLYPHS = ['\u2588', '\u2592', '\u2593', '\u2591', '\u00d7', '#', '%', '\u2573'];
const ORIGINAL_CHARS = ['4', '0', '4'];

interface NotFoundPageProps {
  errMessage?: string;
}

export function NotFoundPage({ errMessage }: NotFoundPageProps = {}) {
  const charRefs = [
    useRef<HTMLSpanElement>(null),
    useRef<HTMLSpanElement>(null),
    useRef<HTMLSpanElement>(null),
  ];

  const buildVersion = resolveBuildVersion();
  const latestBlock = useRealtimeStore((state) => state.latestBlock);
  const blockNumber = latestBlock?.number ?? '---';

  // Lock body scroll on 404 page
  useEffect(() => {
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = '';
    };
  }, []);

  useEffect(() => {
    let timeoutId: ReturnType<typeof setTimeout>;

    function glitchCycle() {
      const idx = Math.floor(Math.random() * 3);
      const el = charRefs[idx].current;
      if (!el) {
        timeoutId = setTimeout(glitchCycle, 4000 + Math.random() * 8000);
        return;
      }

      // First corruption
      el.textContent = GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
      el.style.color = Math.random() < 0.5 ? '#1fb88a' : '#148061';
      const tx = (Math.random() - 0.5) * 4;
      const ty = (Math.random() - 0.5) * 4;
      el.style.transform = `translate(${tx}px, ${ty}px)`;

      // Second corruption after 50ms
      const t1 = setTimeout(() => {
        el.textContent = GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
      }, 50);

      // Restore after 120-200ms
      const restoreDelay = 120 + Math.random() * 80;
      const t2 = setTimeout(() => {
        el.textContent = ORIGINAL_CHARS[idx];
        el.style.color = '#2edba3';
        el.style.transform = 'translate(0, 0)';
      }, restoreDelay);

      // Schedule next glitch — infrequent, mysterious
      timeoutId = setTimeout(glitchCycle, 4000 + Math.random() * 8000);

      // Store sub-timeouts for cleanup
      subTimeouts.push(t1, t2);
    }

    const subTimeouts: ReturnType<typeof setTimeout>[] = [];
    timeoutId = setTimeout(glitchCycle, 2000 + Math.random() * 5000);

    return () => {
      clearTimeout(timeoutId);
      subTimeouts.forEach(clearTimeout);
    };
  }, []);

  return (
    <main className="bg-base-bg relative h-screen overflow-hidden">
      <style>{`
        @keyframes tearScan {
          0% { top: -2px; opacity: 0; }
          2% { opacity: 1; }
          15% { opacity: 1; }
          17% { opacity: 0; }
          100% { top: 100%; opacity: 0; }
        }
        @keyframes screenFlash {
          0%, 100% { background-color: transparent; }
        }
        @keyframes blink {
          50% { opacity: 0; }
        }
        @keyframes flickerLine {
          0%, 90%, 100% { opacity: 1; }
          91% { opacity: 0.3; }
          93% { opacity: 1; }
          94% { opacity: 0.1; }
          96% { opacity: 1; }
        }
      `}</style>

      <NotFoundCellOcean />

      {/* Subtle scan line — single faint sweep */}
      <div className="pointer-events-none fixed inset-0 z-[12]">
        <div
          className="absolute left-0 h-[1px] w-full"
          style={{
            background:
              'linear-gradient(90deg, transparent 0%, rgba(46,219,163,0.05) 30%, rgba(31,184,138,0.04) 70%, transparent 100%)',
            animation: 'tearScan 12s linear infinite',
          }}
        />
      </div>

      {/* Screen flash overlay */}
      <div
        className="pointer-events-none fixed inset-0 z-[13]"
        style={{ animation: 'screenFlash 7s infinite' }}
      />

      <Header />

      {/* Content section */}
      <section className="relative z-20 flex h-full items-center justify-center pb-24">
        <div className="flex flex-col items-center gap-8">
          {/* Glitch 404 text */}
          <div
            className="font-mono font-bold"
            style={{
              fontSize: '120px',
              lineHeight: 1,
              color: '#2edba3',
              textShadow: '0 0 25px rgba(46,219,163,0.35), 0 0 50px rgba(46,219,163,0.15)',
            }}
          >
            <span ref={charRefs[0]} style={{ display: 'inline-block', transition: 'none' }}>
              4
            </span>
            <span ref={charRefs[1]} style={{ display: 'inline-block', transition: 'none' }}>
              0
            </span>
            <span ref={charRefs[2]} style={{ display: 'inline-block', transition: 'none' }}>
              4
            </span>
          </div>

          {/* Poetry block */}
          <div
            className="text-center font-mono"
            style={{ fontSize: '16px', letterSpacing: '0.08em', lineHeight: 2.2 }}
          >
            <p className="text-text-dim">some common knowledge has dissolved into the void</p>
            <p
              style={{
                color: '#2edba3',
                textShadow: '0 0 12px rgba(46,219,163,0.25)',
              }}
            >
              yet more is crystallizing from the chain
            </p>
          </div>
        </div>
      </section>

      {/* Bottom area: error line + footer */}
      <div className="absolute bottom-0 left-0 z-30 flex w-full flex-col items-center gap-3 pb-4">
        {/* ERR line */}
        <div style={{ animation: 'flickerLine 3s infinite' }}>
          <span className="font-mono text-sm">
            <span className="text-rouge">ERR</span>
            <span className="text-text-ghost">
              {' '}
              {errMessage || 'cell_not_found: outpoint unreachable'}
            </span>
            <span
              className="ml-0.5 inline-block"
              style={{
                width: '8px',
                height: '16px',
                backgroundColor: '#2edba3',
                animation: 'blink 1s step-end infinite',
                verticalAlign: 'text-bottom',
              }}
            />
          </span>
        </div>

        {/* Footer — matches SiteFooter style */}
        <div className="flex flex-wrap items-center justify-center gap-3 font-mono text-[11px]">
          <span className="text-text-ghost select-none">&gt;</span>
          <span className="text-text">
            Designed by{' '}
            <a
              href="https://x.com/busyforking"
              target="_blank"
              rel="noreferrer"
              className="text-jade hover:text-jade-dim transition-colors"
            >
              @busyforking
            </a>
            , coded by <span className="text-aqua">Claude</span>
            {' and '}
            <span className="text-aqua">Codex</span>
          </span>
          <span className="text-text-ghost">|</span>
          <span className="text-text-ghost">
            tip: <span className="text-jade/50 tabular-nums">{blockNumber}</span>
          </span>
          <span className="text-text-ghost">|</span>
          <a
            href="https://github.com/janx/ckbadger"
            target="_blank"
            rel="noreferrer"
            className="text-text hover:text-jade transition-colors"
          >
            {buildVersion}
          </a>
          <span className="live-dot" />
          <span className="text-text-ghost">|</span>
          <Link
            href="/hardforks"
            className="text-text hover:text-jade border-base-border hover:border-jade/30 rounded border px-1.5 py-0.5 transition-colors"
          >
            Hardforks
          </Link>
          <a
            href="https://dashboard.fiber.channel/"
            target="_blank"
            rel="noreferrer"
            className="text-text hover:text-jade border-base-border hover:border-jade/30 rounded border px-1.5 py-0.5 transition-colors"
          >
            Fiber Dashboard
          </a>
          <span className="text-text-ghost">|</span>
          <span className="text-text">
            <span className="text-text-dim">?</span> keys
          </span>
        </div>
      </div>
    </main>
  );
}
