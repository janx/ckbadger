'use client';

import { Header } from '@/components/layout/header';
import { HomeContent } from '@/components/home-content';

export default function Home() {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <HomeContent />
    </div>
  );
}
