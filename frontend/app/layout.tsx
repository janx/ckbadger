import { SiteFooter } from '@/components/layout/site-footer';
import './globals.css';
import { Providers } from './providers';

export const metadata = {
  title: 'ckbadger - CKB Blockchain Explorer',
  description: 'High-performance blockchain explorer for Nervos CKB',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="text-text bg-[#08090e] font-mono antialiased">
        <Providers>
          <div className="crt-scanlines" aria-hidden="true" />
          <div className="crt-vignette" aria-hidden="true" />
          <div className="ambient-glow" aria-hidden="true" />
          <div className="flex min-h-screen flex-col">
            <div className="flex-1">{children}</div>
            <SiteFooter />
          </div>
        </Providers>
      </body>
    </html>
  );
}
