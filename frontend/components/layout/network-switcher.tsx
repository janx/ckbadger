'use client';

import { useRouter, usePathname } from '@/src/navigation';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { resolveNetworks } from '@/lib/runtime-config';

/**
 * Header network switcher — a button-group (modeled on `CapacityRangeSelector`)
 * that appears only when 2+ networks are live. Clicking a network navigates to
 * the current page under that network's route prefix.
 *
 * Single-network deployments render nothing. The `resolveNetworks()` gate lives
 * in this outer component (which calls no hooks) so that the single-network path
 * short-circuits WITHOUT invoking navigation hooks — keeping every existing
 * `<Header />` test (which never configures multiple networks) untouched.
 */
export function NetworkSwitcher() {
  const networks = resolveNetworks();
  if (networks.length <= 1) return null;
  return <NetworkSwitcherControl networks={networks} />;
}

function NetworkSwitcherControl({ networks }: { networks: string[] }) {
  const active = useActiveNetwork();
  // `pathname` is canonical / un-prefixed — Task 6's `usePathname` strips the
  // active `:network` segment, so we rebuild the target with the chosen network.
  const pathname = usePathname();
  const router = useRouter();

  // Explicitly network-prefixed target. `useRouter.push` no-ops its auto-prefix
  // because the first segment is already a known network — so no double-prefix.
  const switchTo = (net: string) =>
    router.push(pathname === '/' ? `/${net}` : `/${net}${pathname}`);

  return (
    <div
      data-testid="network-switcher"
      className="border-base-border bg-base-surface/50 flex shrink-0 items-center gap-1 rounded border p-1"
    >
      {networks.map((net) => {
        const isActive = net === active;
        return (
          <button
            key={net}
            type="button"
            onClick={() => switchTo(net)}
            aria-current={isActive ? 'page' : undefined}
            aria-label={`Switch to ${net}`}
            className={`rounded px-2 py-1 font-mono text-xs capitalize transition-colors ${
              isActive
                ? 'bg-jade/15 text-jade border-jade-dim border'
                : 'text-text-dim hover:bg-base-elevated hover:text-text-bright'
            }`}
          >
            {net}
          </button>
        );
      })}
    </div>
  );
}
