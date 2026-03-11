'use client';

import { useEffect, useRef } from 'react';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { NotFoundCellOcean } from '@/components/not-found-cell-ocean';

const GLYPHS = ['\u2588', '\u2592', '\u2593', '\u2591', '\u00d7', '#', '%', '\u2573'];
const ORIGINAL_CHARS = ['4', '0', '4'];

export function NotFoundPage() {
  const charRefs = [
    useRef<HTMLSpanElement>(null),
    useRef<HTMLSpanElement>(null),
    useRef<HTMLSpanElement>(null),
  ];

  useEffect(() => {
    let timeoutId: ReturnType<typeof setTimeout>;

    function glitchCycle() {
      const idx = Math.floor(Math.random() * 3);
      const el = charRefs[idx].current;
      if (!el) {
        timeoutId = setTimeout(glitchCycle, 2000 + Math.random() * 4000);
        return;
      }

      // First corruption
      el.textContent = GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
      el.style.color = Math.random() < 0.5 ? '#e8555a' : '#68ccf0';
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

      // Schedule next glitch
      timeoutId = setTimeout(glitchCycle, 2000 + Math.random() * 4000);

      // Store sub-timeouts for cleanup
      subTimeouts.push(t1, t2);
    }

    const subTimeouts: ReturnType<typeof setTimeout>[] = [];
    timeoutId = setTimeout(glitchCycle, 1000 + Math.random() * 2000);

    return () => {
      clearTimeout(timeoutId);
      subTimeouts.forEach(clearTimeout);
    };
  }, []);

  return (
    <main className="bg-base-bg relative min-h-screen overflow-hidden">
      <style>{`
        @keyframes tearScan {
          0% { top: -2px; opacity: 0; }
          2% { opacity: 1; }
          15% { opacity: 1; }
          17% { opacity: 0; }
          100% { top: 100%; opacity: 0; }
        }
        @keyframes screenFlash {
          0%, 92%, 100% { background-color: transparent; }
          93% { background-color: rgba(46, 219, 163, 0.03); }
          93.5% { background-color: transparent; }
          94% { background-color: rgba(232, 85, 90, 0.04); }
          94.5% { background-color: transparent; }
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

      {/* Screen tear overlay */}
      <div className="pointer-events-none fixed inset-0 z-[12]">
        <div
          className="absolute left-0 h-[2px] w-full"
          style={{
            background:
              'linear-gradient(90deg, transparent 0%, rgba(46,219,163,0.15) 20%, rgba(104,204,240,0.1) 50%, rgba(46,219,163,0.15) 80%, transparent 100%)',
            animation: 'tearScan 8s linear infinite',
          }}
        />
        <div
          className="absolute left-0 h-[1px] w-full"
          style={{
            background:
              'linear-gradient(90deg, transparent 0%, rgba(232,85,90,0.12) 30%, rgba(232,85,90,0.08) 70%, transparent 100%)',
            animation: 'tearScan 8s linear infinite',
            animationDelay: '2.8s',
          }}
        />
        <div
          className="absolute left-0 h-[1px] w-full"
          style={{
            background:
              'linear-gradient(90deg, transparent 0%, rgba(46,219,163,0.08) 40%, rgba(104,204,240,0.06) 60%, transparent 100%)',
            animation: 'tearScan 8s linear infinite',
            animationDelay: '5.3s',
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
      <section className="relative z-20 flex min-h-screen items-center justify-center">
        <div className="flex flex-col items-center gap-8">
          {/* Glitch 404 text */}
          <div
            className="font-mono font-bold"
            style={{
              fontSize: '120px',
              lineHeight: 1,
              color: '#2edba3',
              textShadow: '0 0 20px rgba(46,219,163,0.4), 0 0 40px rgba(46,219,163,0.2)',
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
                textShadow: '0 0 10px rgba(46,219,163,0.3)',
              }}
            >
              yet more is crystallizing from the chain
            </p>
          </div>

          {/* Return Home link */}
          <Link
            href="/"
            className="border-jade-dim bg-jade/10 text-jade hover:bg-jade/20 rounded-md border px-5 py-2.5 font-mono text-xs uppercase tracking-[0.18em] transition"
          >
            Return Home
          </Link>
        </div>
      </section>

      {/* Fixed bottom error line */}
      <div
        className="fixed bottom-12 left-0 z-30 flex w-full items-center justify-center"
        style={{ animation: 'flickerLine 3s infinite' }}
      >
        <span className="font-mono text-sm">
          <span className="text-rouge">ERR</span>
          <span className="text-text-ghost"> cell_not_found: outpoint unreachable</span>
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
    </main>
  );
}
