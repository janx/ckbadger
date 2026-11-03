import { Atmosphere } from '@/components/layout/atmosphere';
import { SiteFooter } from '@/components/layout/site-footer';
import './globals.css';
import { Providers } from './providers';

export const metadata = {
  title: 'CKBadger — Local-first CKB-native Explorer',
  description:
    'Local-first, agent-friendly Nervos CKB explorer. All you need is a CKB node and the will to run your own stack.',
};

/**
 * The Next-era shell. The SPA's entry is src/main.tsx → AppRoot → the router, whose AppFrame
 * (src/routes/router.tsx) is the page root that is actually rendered; this file is kept in step
 * with it (the same Atmosphere) but is not on the render path.
 */
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="text-text bg-[#08090e] font-mono antialiased">
        <Providers>
          <Atmosphere />
          <div className="flex min-h-screen flex-col">
            <div className="flex-1">{children}</div>
            <SiteFooter />
          </div>
        </Providers>
      </body>
    </html>
  );
}
