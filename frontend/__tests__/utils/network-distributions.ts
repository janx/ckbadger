import type { LabelCount, NetworkDistributions } from '@/lib/api';

/**
 * A network distributions fixture as the route sends it (lane exactness, part A, A5 iii): a
 * test writes the histograms, and this adds the two groupings `/network/distributions` makes
 * from the same nodes — `hosting`, the verified nodes by organisation (an ASN label's text after
 * its number; `Unknown` as it is), the five largest by nodes, ties by name in code-unit order
 * (the route's byte order on these labels), and the rest; and `releases`, the versions by the
 * label before their build. The page reads them and sums nothing.
 */
const byCount = (a: { count: number; key: string }, b: { count: number; key: string }) =>
  b.count - a.count || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0);

function organisation(label: string): string {
  if (label === 'Unknown') return label;
  const match = /^AS\d+ (.+)$/.exec(label);
  if (!match) throw new Error(`fixture: unexpected ASN label ${label}`);
  return match[1];
}

export function withGroupings(
  d: Omit<NetworkDistributions, 'hosting' | 'releases'> &
    Partial<Pick<NetworkDistributions, 'hosting' | 'releases'>>
): NetworkDistributions {
  const orgs = new Map<string, number>();
  for (const asn of d.asns)
    orgs.set(organisation(asn.label), (orgs.get(organisation(asn.label)) ?? 0) + asn.count);
  const ranked = [...orgs.entries()]
    .map(([key, count]) => ({ key, count }))
    .sort(byCount)
    .map(({ key, count }): LabelCount => ({ label: key, count }));
  const rest = ranked.slice(5);
  const releases = new Map<string, LabelCount[]>();
  for (const build of [...d.versions]
    .map((v) => ({ key: v.label, count: v.count }))
    .sort(byCount)) {
    const release = build.key.split(' ')[0];
    releases.set(release, [
      ...(releases.get(release) ?? []),
      { label: build.key, count: build.count },
    ]);
  }
  return {
    ...d,
    hosting: {
      organisations: ranked.slice(0, 5),
      rest: rest.reduce((sum, row) => sum + row.count, 0),
      restOrganisations: rest.length,
    },
    releases: [...releases.entries()]
      .map(([release, builds]) => ({
        release,
        count: builds.reduce((sum, b) => sum + b.count, 0),
        builds,
      }))
      .sort((a, b) => b.count - a.count || (a.release < b.release ? -1 : 1)),
  };
}
