'use client';

import Image from '@/components/ui/image';
import Link from '@/components/ui/link';
import { useEffect, useRef } from 'react';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

export function Logo() {
  const blockNumber = useRealtimeStore((state) => state.latestBlock?.number);
  const imgRef = useRef<HTMLImageElement>(null);
  const seenRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (blockNumber === undefined) return;
    if (seenRef.current === blockNumber) return;
    seenRef.current = blockNumber;

    const el = imgRef.current;
    if (!el) return;
    el.classList.remove('neon-flicker');
    // Force reflow so the next class add restarts the animation even if
    // the previous flicker is still mid-run.
    void el.offsetWidth;
    el.classList.add('neon-flicker');

    const timer = setTimeout(() => {
      el.classList.remove('neon-flicker');
    }, 820);
    return () => clearTimeout(timer);
  }, [blockNumber]);

  return (
    <Link
      href="/"
      className="logo-container group absolute -left-[16px] -top-[13px] z-[9999] md:-left-[22px] md:-top-[7px]"
      aria-label="CKBadger Home"
      title="Hi, I'm the ckbadger Shannon!"
    >
      <Image
        ref={imgRef}
        src="/ckbadger-logo.webp"
        alt="CKBadger"
        width={143}
        height={97}
        unoptimized
        priority
        className="logo-image h-auto w-[135px] rotate-[8deg] transform-gpu object-contain transition-[transform,filter] duration-300 group-hover:rotate-[9deg] md:w-[157px]"
      />
    </Link>
  );
}
