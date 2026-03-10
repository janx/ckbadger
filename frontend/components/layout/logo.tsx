'use client';

import Link from '@/components/ui/link';
import { useEffect, useState } from 'react';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

export function Logo() {
  const latestBlock = useRealtimeStore((state) => state.latestBlock);
  const [isGlitching, setIsGlitching] = useState(false);

  useEffect(() => {
    if (latestBlock) {
      setIsGlitching(true);
      const timer = setTimeout(() => setIsGlitching(false), 800);
      return () => clearTimeout(timer);
    }
  }, [latestBlock]);

  return (
    <Link
      href="/"
      className={`group flex items-center gap-0 font-mono ${isGlitching ? 'logo-glow-flicker' : 'logo-glow'}`}
      aria-label="CKBadger Home"
    >
      <span className="text-interactive group-hover:text-interactive-hover mr-1.5 text-sm font-normal transition-colors md:text-base">
        $
      </span>
      <span
        className={`text-emphasis text-sm font-bold tracking-tight transition-all md:text-base ${isGlitching ? 'logo-flicker' : ''}`}
      >
        ckbadger
      </span>
      <span
        className={`bg-interactive group-hover:bg-interactive-hover ml-0.5 inline-block h-[1.1em] w-[2px] ${isGlitching ? 'logo-cursor-flash' : 'opacity-0'}`}
      />
    </Link>
  );
}
