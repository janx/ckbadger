'use client';

import { useRouter, usePathname } from '@/src/navigation';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { resolveNetworks } from '@/lib/runtime-config';
import { cn } from '@/lib/utils';
import {
  NAVBAR_DROPDOWN_ITEM_ACTIVE_CLASS,
  NAVBAR_DROPDOWN_ITEM_CLASS,
  NAVBAR_DROPDOWN_ITEM_DEFAULT_CLASS,
  NAVBAR_DROPDOWN_PANEL_CLASS,
  NAVBAR_DROPDOWN_POPOVER_CLASS,
  NAVBAR_DROPDOWN_TRIGGER_CLASS,
  NAVBAR_DROPDOWN_TRIGGER_DEFAULT_CLASS,
} from '@/components/layout/navbar-dropdown-styles';

/**
 * Header network selector. It appears only when 2+ networks are live and keeps
 * the user on the same page when switching the route's network prefix.
 *
 * Single-network deployments render nothing. The `resolveNetworks()` gate lives
 * in this outer component (which calls no hooks) so that the single-network path
 * short-circuits WITHOUT invoking navigation hooks — keeping every existing
 * `<Header />` test (which never configures multiple networks) untouched.
 */
interface NetworkSwitcherProps {
  className?: string;
  onSwitch?: () => void;
}

export function NetworkSwitcher({ className, onSwitch }: NetworkSwitcherProps = {}) {
  const networks = resolveNetworks();
  if (networks.length <= 1) return null;
  return <NetworkSwitcherControl networks={networks} className={className} onSwitch={onSwitch} />;
}

interface NetworkSwitcherControlProps extends NetworkSwitcherProps {
  networks: string[];
}

function NetworkSwitcherControl({ networks, className, onSwitch }: NetworkSwitcherControlProps) {
  const active = useActiveNetwork();
  // `pathname` is canonical / un-prefixed — Task 6's `usePathname` strips the
  // active `:network` segment, so we rebuild the target with the chosen network.
  const pathname = usePathname();
  const router = useRouter();

  // Explicitly network-prefixed target. `useRouter.push` no-ops its auto-prefix
  // because the first segment is already a known network — so no double-prefix.
  const switchTo = (net: string) => {
    router.push(pathname === '/' ? `/${net}` : `/${net}${pathname}`);
    onSwitch?.();
  };

  return (
    <div data-testid="network-switcher" className={cn('group relative shrink-0', className)}>
      <button
        type="button"
        aria-label="Select network"
        aria-haspopup="menu"
        className={cn(NAVBAR_DROPDOWN_TRIGGER_CLASS, NAVBAR_DROPDOWN_TRIGGER_DEFAULT_CLASS)}
      >
        {active}
        <svg
          aria-hidden="true"
          className="h-3 w-3 opacity-50"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>
      <div className={NAVBAR_DROPDOWN_POPOVER_CLASS}>
        <div className={NAVBAR_DROPDOWN_PANEL_CLASS}>
          {networks.map((net) => {
            const isActive = net === active;
            return (
              <button
                key={net}
                type="button"
                aria-label={`Switch to ${net}`}
                aria-current={isActive ? 'page' : undefined}
                onClick={() => switchTo(net)}
                className={cn(
                  NAVBAR_DROPDOWN_ITEM_CLASS,
                  isActive ? NAVBAR_DROPDOWN_ITEM_ACTIVE_CLASS : NAVBAR_DROPDOWN_ITEM_DEFAULT_CLASS
                )}
              >
                {net}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
