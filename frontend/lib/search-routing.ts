export function resolveSearchRoute(input: string): string {
  const trimmed = input.trim();

  if (/^[0-9]+$/.test(trimmed)) {
    return `/blocks/${trimmed}`;
  }

  if (/^0x[a-fA-F0-9]{64}$/.test(trimmed)) {
    return `/tx/${trimmed}`;
  }

  if (trimmed.startsWith('ckb') || trimmed.startsWith('ckt')) {
    return `/address/${trimmed}`;
  }

  if (trimmed.includes('-')) {
    return `/cell/${trimmed}`;
  }

  return `/search?q=${encodeURIComponent(trimmed)}`;
}
