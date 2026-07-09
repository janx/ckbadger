import {
  resolveApiBasePattern,
  resolveDefaultNetwork,
  resolveNetworks,
  resolveWsUrlPattern,
} from '@/lib/runtime-config';

export function isKnownNetwork(name: string): boolean {
  return resolveNetworks().includes(name);
}

export function networkFromPath(pathname: string): string | null {
  const seg = pathname.replace(/^\/+/, '').split('/')[0] ?? '';
  return seg && isKnownNetwork(seg) ? seg : null;
}

export function resolveActiveNetwork(
  pathname = typeof window !== 'undefined' ? window.location.pathname : '/'
): string {
  return networkFromPath(pathname) ?? resolveDefaultNetwork();
}

export function apiBaseFor(network: string): string {
  return resolveApiBasePattern().replace('{network}', network);
}

export function wsUrlFor(network: string): string {
  const origin =
    typeof window !== 'undefined'
      ? window.location.origin.replace(/^http/, 'ws')
      : 'ws://localhost:8100';
  return `${origin}${resolveWsUrlPattern().replace('{network}', network)}`;
}
