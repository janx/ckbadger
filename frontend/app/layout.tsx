import { SiteFooter } from '@/components/layout/site-footer';
import './globals.css';
import { Providers } from './providers';

export const metadata = {
  title: 'ckbadger - CKB Blockchain Explorer',
  description: 'High-performance blockchain explorer for Nervos CKB',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="dark">
      <body className="font-sans antialiased">
        <Providers>
          <div className="flex min-h-screen flex-col">
            <div className="flex-1">{children}</div>
            <SiteFooter />
          </div>
        </Providers>
      </body>
    </html>
  );
}
