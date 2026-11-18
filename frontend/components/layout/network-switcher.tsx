'use client';

import { useRouter, usePathname, useSearchAndHash } from '@/src/navigation';
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
  /**
   * Where it stands: in the header's nav row (`nav`: a trigger with its menu dropping below it,
   * set off from the nav's links by a line), or in the header's menu (`menu`: a full-width row
   * whose list opens in place). The header puts it in one or the other by the row's form.
   */
  placement?: 'nav' | 'menu';
}

/** Each placement's classes: the wrapper, the trigger, the list's frame, the list's inner box. */
const PLACEMENT = {
  nav: {
    wrapper: 'border-base-border/70 mr-1 w-auto border-r pr-2',
    trigger: 'w-auto',
    frame:
      'invisible absolute left-0 top-full block w-auto translate-y-1 pt-2 transition-all group-focus-within:visible group-focus-within:translate-y-0 group-hover:visible group-hover:translate-y-0',
    inner: 'overflow-visible pt-0',
  },
  menu: {
    wrapper: 'w-full',
    trigger: 'w-full',
    frame:
      'grid w-full grid-rows-[0fr] transition-[grid-template-rows,opacity] group-focus-within:grid-rows-[1fr] group-hover:grid-rows-[1fr]',
    inner: 'overflow-hidden pt-2',
  },
} as const;

export function NetworkSwitcher({
  className,
  onSwitch,
  placement = 'nav',
}: NetworkSwitcherProps = {}) {
  const networks = resolveNetworks();
  if (networks.length <= 1) return null;
  return (
    <NetworkSwitcherControl
      networks={networks}
      className={className}
      onSwitch={onSwitch}
      placement={placement}
    />
  );
}

interface NetworkSwitcherControlProps extends NetworkSwitcherProps {
  networks: string[];
  placement: 'nav' | 'menu';
}

function NetworkSwitcherControl({
  networks,
  className,
  onSwitch,
  placement,
}: NetworkSwitcherControlProps) {
  const at = PLACEMENT[placement];
  const active = useActiveNetwork();
  // `pathname` is canonical / un-prefixed — Task 6's `usePathname` strips the
  // active `:network` segment, so we rebuild the target with the chosen network.
  const pathname = usePathname();
  // `usePathname` carries no search/hash; keep them so a switch preserves the
  // user's filters and anchor, exactly as NetworkGuard does when it prefixes a path.
  const searchAndHash = useSearchAndHash();
  const router = useRouter();

  // Explicitly network-prefixed target. `useRouter.push` no-ops its auto-prefix
  // because the first segment is already a known network — so no double-prefix.
  const switchTo = (net: string) => {
    const basePath = pathname === '/' ? `/${net}` : `/${net}${pathname}`;
    router.push(`${basePath}${searchAndHash}`);
    onSwitch?.();
  };

  return (
    <div
      data-testid="network-switcher"
      data-control="network-context"
      data-placement={placement}
      className={cn('group relative shrink-0', at.wrapper, className)}
    >
      <button
        type="button"
        aria-label="Select network"
        aria-haspopup="menu"
        className={cn(
          NAVBAR_DROPDOWN_TRIGGER_CLASS,
          'border-aqua/50 bg-aqua/10 text-aqua shadow-glow-aqua hover:border-aqua/80 hover:bg-aqua/15 hover:text-info-bright focus-visible:border-aqua focus-visible:ring-aqua/30 justify-between focus-visible:outline-none focus-visible:ring-1',
          at.trigger
        )}
      >
        <span
          aria-hidden="true"
          className="bg-aqua h-1.5 w-1.5 shrink-0 rounded-full shadow-[0_0_6px_rgb(var(--rgb-aqua)/calc(0.8*var(--m-glow)))]"
        />
        <span className="truncate">{active}</span>
        <svg
          aria-hidden="true"
          className="text-aqua-dim ml-auto h-3 w-3 shrink-0"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>
      <div
        className={cn(
          'z-50 opacity-0 duration-150 group-focus-within:opacity-100 group-hover:opacity-100',
          at.frame
        )}
      >
        <div className={cn('min-h-0', at.inner)}>
          <div
            role="menu"
            aria-label="CKB network"
            className="border-base-border bg-base-surface min-w-[11rem] overflow-hidden border shadow-lg"
          >
            <div className="eyebrow border-base-border/70 text-text-dim text-meta border-b px-3 py-2 font-mono">
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
                      'flex w-full items-center gap-2 px-2.5 py-2 text-left transition-colors',
                      isActive
                        ? 'bg-aqua/10 text-aqua'
                        : 'text-text-dim hover:bg-base-elevated/60 hover:text-text'
                    )}
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        'flex h-3 w-3 shrink-0 items-center justify-center rounded-full border',
                        isActive ? 'border-aqua' : 'border-text-ghost'
                      )}
                    >
                      {isActive && <span className="bg-aqua h-1 w-1 rounded-full" />}
                    </span>
                    <span className="eyebrow text-meta min-w-0 flex-1 truncate font-mono">
                      {net}
                    </span>
                    {isActive && (
                      <span className="eyebrow text-aqua-dim text-meta font-mono">Current</span>
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
