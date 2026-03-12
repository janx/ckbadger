'use client';

import Image from '@/components/ui/image';
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
      className="logo-container group absolute -left-[8px] -top-[28px] z-[9999] md:-left-[14px] md:-top-[22px]"
      aria-label="CKBadger Home"
      title="Hi, I'm the ckbadger Shannon!"
    >
      <Image
        src="/ckbadger-transparent.png"
        alt="CKBadger"
        width={143}
        height={97}
        unoptimized
        priority
        className={`logo-image h-auto w-[96px] rotate-[8deg] transform-gpu object-contain transition-all duration-300 group-hover:rotate-[9deg] group-hover:scale-100 md:w-[112px] ${isGlitching ? 'neon-flicker' : ''}`}
      />
    </Link>
  );
}
