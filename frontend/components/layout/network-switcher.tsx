'use client';

import { useRouter, usePathname } from '@/src/navigation';
import { NAVBAR_DROPDOWN_TRIGGER_CLASS } from '@/components/layout/navbar-dropdown-styles';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { resolveNetworks } from '@/lib/runtime-config';
import { cn } from '@/lib/utils';

/**
 * Header chain-context selector. It appears only when 2+ networks are live and
 * keeps the user on the same page when switching the route's network prefix.
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
    <div
      data-testid="network-switcher"
      data-control="network-context"
      className={cn(
        'md:border-base-border/70 group relative w-full shrink-0 md:mr-1 md:w-auto md:border-r md:pr-2',
        className
      )}
    >
      <button
        type="button"
        aria-label="Select network"
        aria-haspopup="menu"
        className={cn(
          NAVBAR_DROPDOWN_TRIGGER_CLASS,
          'border-base-border/80 bg-base-surface/70 text-text-dim hover:border-text-ghost hover:bg-base-elevated/70 hover:text-text focus-visible:border-aqua/40 focus-visible:ring-aqua/20 w-full justify-between focus-visible:outline-none focus-visible:ring-1 md:w-auto'
        )}
      >
        <span aria-hidden="true" className="bg-text-ghost h-1.5 w-1.5 shrink-0 rounded-full" />
        <span className="truncate">{active}</span>
        <svg
          aria-hidden="true"
          className="text-text-ghost ml-auto h-3 w-3 shrink-0"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>
      <div className="z-50 grid w-full grid-rows-[0fr] opacity-0 transition-[grid-template-rows,opacity] duration-150 group-focus-within:grid-rows-[1fr] group-focus-within:opacity-100 group-hover:grid-rows-[1fr] group-hover:opacity-100 md:invisible md:absolute md:left-0 md:top-full md:block md:w-auto md:translate-y-1 md:pt-2 md:transition-all md:group-focus-within:visible md:group-focus-within:translate-y-0 md:group-hover:visible md:group-hover:translate-y-0">
        <div className="min-h-0 overflow-hidden pt-2 md:overflow-visible md:pt-0">
          <div
            role="menu"
            aria-label="CKB network"
            className="border-base-border bg-base-surface min-w-[11rem] overflow-hidden rounded-md border shadow-lg"
          >
            <div className="border-base-border/70 text-text-ghost border-b px-3 py-2 font-mono text-[9px] uppercase tracking-[0.14em]">
              CKB network
            </div>
            <div className="p-1.5">
              {networks.map((net) => {
                const isActive = net === active;
                return (
                  <button
                    key={net}
                    type="button"
                    role="menuitemradio"
                    aria-label={`Switch to ${net}`}
                    aria-checked={isActive}
                    aria-current={isActive ? 'page' : undefined}
                    onClick={() => switchTo(net)}
                    className={cn(
                      'flex w-full items-center gap-2 rounded-sm px-2.5 py-2 text-left transition-colors',
                      isActive
                        ? 'bg-base-elevated text-text-bright'
                        : 'text-text-dim hover:bg-base-elevated/60 hover:text-text'
                    )}
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        'flex h-3 w-3 shrink-0 items-center justify-center rounded-full border',
                        isActive ? 'border-aqua/70' : 'border-text-ghost'
                      )}
                    >
                      {isActive && <span className="bg-aqua/70 h-1 w-1 rounded-full" />}
                    </span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[11px] uppercase tracking-[0.12em]">
                      {net}
                    </span>
                    {isActive && (
                      <span className="text-text-ghost font-mono text-[8px] uppercase tracking-[0.12em]">
                        Current
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
