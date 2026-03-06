'use client';

import { AppLink } from '@/components/ui/app-link';

interface LinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
  href: string;
  replace?: boolean;
  prefetch?: boolean;
  scroll?: boolean;
}

export default function Link({
  href,
  replace,
  prefetch: _prefetch,
  scroll: _scroll,
  ...props
}: LinkProps) {
  return <AppLink href={href} replace={replace} {...props} />;
}
