'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type NetworkMap } from '@/lib/api';
import { useElementWidth } from '@/hooks/useElementWidth';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatUtc8Date } from '@/lib/utils';

type Point = NetworkMap['points'][number];

const DOT_R = 3.6;
const RING_R = 6.2;

/** Dot spacing inside a country cluster, and the map's latitude band (the Network board). */
const SPACING = 9.2;
const LAT_TOP = 72;
const LAT_BOTTOM = -48;
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

export interface CountryLabel {
  country: string;
  count: number;
  x: number;
  y: number;
}

/**
 * The Network board's layout: one cluster per country at the mean GeoIP position of its peers
 * (lat 72…−48 band), clusters pushed apart until they and their labels no longer touch and pulled
 * gently back to their place, peers on a sunflower inside — ringed first, then reachable. Visual
 * displacement only: the readout keeps the source coordinates. Deterministic.
 */
export function layoutCountryClusters(
  points: Point[],
  width: number,
  height: number
): { dots: Array<{ point: Point; x: number; y: number }>; labels: CountryLabel[] } {
  if (!(width > 0 && height > 0)) throw new Error(`invalid network map size ${width}×${height}`);
  const proj = (lon: number, lat: number): [number, number] => [
    ((lon + 180) / 360) * width,
    ((LAT_TOP - Math.max(LAT_BOTTOM, Math.min(LAT_TOP, lat))) / (LAT_TOP - LAT_BOTTOM)) * height,
  ];
  const byCountry = new Map<string, Point[]>();
  for (const point of points)
    byCountry.set(point.country, [...(byCountry.get(point.country) ?? []), point]);
  const countries = [...byCountry.keys()].sort();
  const radius = (n: number) => SPACING * (0.6 + Math.sqrt(n) * 0.62);
  const home = new Map(
    countries.map((country) => {
      const peers = byCountry.get(country)!;
      return [
        country,
        proj(
          peers.reduce((sum, p) => sum + p.lon, 0) / peers.length,
          peers.reduce((sum, p) => sum + p.lat, 0) / peers.length
        ),
      ];
    })
  );
  const centre = new Map(countries.map((country) => [country, [...home.get(country)!]]));
  /** A cluster's footprint: its dots plus the label under it. */
  const extent = (country: string) => {
    const r = radius(byCountry.get(country)!.length);
    const half = Math.max(r, (`${country} ${byCountry.get(country)!.length}`.length * 6.2) / 2);
    return { r, half, below: r + 14 };
  };
  for (let round = 0; round < 400; round++) {
    for (let i = 0; i < countries.length; i++)
      for (let j = i + 1; j < countries.length; j++) {
        const a = centre.get(countries[i])!;
        const b = centre.get(countries[j])!;
        const ea = extent(countries[i]);
        const eb = extent(countries[j]);
        const dx = b[0] - a[0];
        const dy = b[1] - a[1];
        const d = Math.hypot(dx, dy) || 0.01;
        const need = Math.max(ea.r + eb.r, ea.half + eb.half) + 12;
        if (d < need) {
          const m = (need - d) / 2;
          const ux = dx === 0 && dy === 0 ? 1 : dx / d;
          const uy = dx === 0 && dy === 0 ? 0 : dy / d;
          a[0] -= ux * m;
          a[1] -= uy * m;
          b[0] += ux * m;
          b[1] += uy * m;
        }
      }
    for (const country of countries) {
      const c = centre.get(country)!;
      const [tx, ty] = home.get(country)!;
      const e = extent(country);
      c[0] += (tx - c[0]) * 0.02;
      c[1] += (ty - c[1]) * 0.02;
      c[0] = Math.max(e.half, Math.min(width - e.half, c[0]));
      c[1] = Math.max(e.r, Math.min(height - e.below, c[1]));
    }
  }
  const dots: Array<{ point: Point; x: number; y: number }> = [];
  const labels: CountryLabel[] = [];
  for (const country of countries) {
    const [cx, cy] = centre.get(country)!;
    const peers = [...byCountry.get(country)!].sort(
      (a, b) =>
        Number(b.localSessionObserved) - Number(a.localSessionObserved) ||
        Number(b.reachable) - Number(a.reachable) ||
        a.peerId.localeCompare(b.peerId)
    );
    peers.forEach((point, k) => {
      const r = peers.length > 1 ? SPACING * 0.62 * Math.sqrt(k + 0.5) : 0;
      dots.push({
        point,
        x: cx + r * Math.cos(k * GOLDEN_ANGLE),
        y: cy + r * Math.sin(k * GOLDEN_ANGLE),
      });
    });
    labels.push({ country, count: peers.length, x: cx, y: cy + radius(peers.length) + 11 });
  }
  dots.sort((a, b) => a.point.peerId.localeCompare(b.point.peerId));
  return { dots, labels };
}

/** The dots alone, on the default 720 × 353 map (kept for callers that place only dots). */
export function layoutMapPoints(
  points: Point[],
  width = 720,
  height = Math.round(720 * 0.49)
): Array<{ point: Point; x: number; y: number }> {
  return layoutCountryClusters(points, width, height).dots;
}

/** A peer id is a 34-byte multihash: head…tail is enough to tell peers apart (CH-57). */
export function shortPeerId(peerId: string): string {
  return peerId.length > 20 ? `${peerId.slice(0, 10)}…${peerId.slice(-6)}` : peerId;
}

function Legend({
  reachable,
  before,
  ringed,
  neverVerified,
}: {
  reachable: number;
  before: number;
  ringed: number;
  neverVerified: number;
}) {
  const row = (glyph: React.ReactNode, label: string) => (
    <span className="flex items-center gap-2">
      <svg aria-hidden="true" width="14" height="14">
        {glyph}
      </svg>
      {label}
    </span>
  );
  return (
    <>
      <p className="text-text-bright text-[13.5px]">Hover a node</p>
      <div className="mt-2.5 flex flex-col gap-1.5">
        {row(<circle cx="7" cy="7" r={DOT_R} fill={DATA_COLOR} />, `reachable now · ${reachable}`)}
        {row(
          <circle cx="7" cy="7" r="3.3" fill="#0c0e15" stroke="#dee2ec" />,
          `verified before, not now · ${before}`
        )}
        {row(
          <>
            <circle cx="7" cy="7" r="6" fill="none" stroke="#dee2ec" strokeWidth="1.2" />
            <circle cx="7" cy="7" r={DOT_R} fill={DATA_COLOR} />
          </>,
          `this node's ${ringed} session peers`
        )}
        {row(
          <circle cx="7" cy="7" r="1.8" fill="#4b5266" />,
          `heard of, never verified · ${neverVerified}`
        )}
      </div>
      <p className="text-text-dim mt-2.5 leading-relaxed">
        Each node keeps its own handful of connections to others. Those links cannot be seen from
        outside, so none are drawn: not even this node&apos;s, which would make it look like a
        centre.
      </p>
    </>
  );
}

function PeerReadout({ point, roundId }: { point: Point; roundId: number | null }) {
  const rows: [string, string, string?][] = [
    [
      'state',
      point.reachable ? 'reachable now' : 'verified before, not reachable now',
      point.reachable ? 'text-jade' : undefined,
    ],
    [
      'to this node',
      point.localSessionObserved
        ? 'a session in the latest round'
        : 'no session in the latest round',
    ],
    ['version', point.clientVersion],
    ['where', `${point.country} · ${point.asn ?? '—'}`],
    [
      'in address books',
      `of ${point.advertiserCount.toLocaleString()} peers in round #${roundId ?? '—'}`,
    ],
    ['first seen', formatUtc8Date(point.firstSeen * 1000)],
  ];
  return (
    <>
      <p className="text-text-bright text-[13.5px]">peer {shortPeerId(point.peerId)}</p>
      <dl className="mt-2 grid grid-cols-[7rem_minmax(0,1fr)] gap-x-2 gap-y-1">
        {rows.map(([label, value, tone]) => (
          <div key={label} className="contents">
            <dt className="text-text-dim">{label}</dt>
            <dd className={`[overflow-wrap:anywhere] ${tone ?? 'text-text'}`}>{value}</dd>
          </div>
        ))}
      </dl>
    </>
  );
}

/**
 * Where the verified nodes are (Network board V70): every verified peer a dot at its GeoIP place,
 * reachable solid, verified-before hollow, this node's session peers ringed wherever they are; the
 * never-verified peers a strip beneath. No link is drawn and nothing is centred (user correction).
 */
export function NetworkMapView({ neverVerified }: { neverVerified: number }) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [ref, measured] = useElementWidth<HTMLDivElement>();
  const { data, isLoading, error } = useQuery({
    queryKey: ['network', 'map'],
    queryFn: api.getNetworkMap,
    refetchInterval: 30000,
  });
  if (!Number.isSafeInteger(neverVerified) || neverVerified < 0)
    throw new Error(`invalid never-verified peer count ${neverVerified}`);
  const width = measured ?? 720;
  // The board's map is about twice as wide as it is tall (880 × 430).
  // On a phone the clusters need more room than the board's ratio leaves (390 shot).
  const height = Math.max(220, Math.round(width * 0.49));
  const ready = !isLoading && !error && data ? data : null;
  const placed = ready ? layoutCountryClusters(ready.points, width, height) : null;
  const layout = placed?.dots ?? [];
  const selected = ready?.points.find((point) => point.peerId === selectedId) ?? null;
  const countryLabels = placed?.labels ?? [];
  const reachable = ready?.points.filter((point) => point.reachable).length ?? 0;
  const ringed = ready?.points.filter((point) => point.localSessionObserved).length ?? 0;
  const perRow = Math.max(8, Math.floor(width / 14));
  return (
    <figure className="m-0 min-w-0">
      {ready && (
        <p className="text-text-dim mb-3 text-[12px]">
          {ready.located.toLocaleString()} verified nodes in {countryLabels.length} countries ·
          ringed: the {ringed} this node had a session with in the latest round
        </p>
      )}
      <div className="grid min-w-0 gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
        {/* Measured from mount, so the map is drawn 1:1 in its own column at every width. */}
        <div ref={ref} className="flex min-w-0 flex-col gap-3">
          {isLoading ? (
            <p className="text-text-dim text-[12px]">Loading crawler locations…</p>
          ) : !ready ? (
            <p role="alert" className="text-negative text-[12px]">
              Crawler locations unavailable: {String(error)}
            </p>
          ) : (
            <>
              <svg
                width={width}
                height={height}
                viewBox={`0 0 ${width} ${height}`}
                role="img"
                aria-label="Approximate GeoIP locations of retained verified CKB peers"
                className="block"
                onMouseLeave={() => setSelectedId(null)}
              >
                {[1, 2, 3, 4, 5, 6, 7].map((k) => (
                  <line
                    key={'lon' + k}
                    x1={(width * k) / 8}
                    x2={(width * k) / 8}
                    y1="0"
                    y2={height}
                    stroke="#121620"
                  />
                ))}
                {[1, 2, 3, 4, 5].map((k) => (
                  <line
                    key={'lat' + k}
                    x1="0"
                    x2={width}
                    y1={(height * k) / 6}
                    y2={(height * k) / 6}
                    stroke={k === 3 ? '#1a1f30' : '#121620'}
                  />
                ))}
                {countryLabels.map(({ country, count, x, y }) => (
                  <text
                    key={country}
                    data-country-label
                    x={x}
                    y={y}
                    textAnchor="middle"
                    fontSize="10"
                    fill="#7c849c"
                    stroke="#0c0e15"
                    strokeWidth="3"
                    paintOrder="stroke"
                  >
                    {country} {count}
                  </text>
                ))}
                {layout.map(({ point, x, y }) => (
                  <g
                    key={point.peerId}
                    data-peer={point.peerId}
                    tabIndex={0}
                    aria-label={`${point.country} peer ${shortPeerId(point.peerId)}, ${
                      point.reachable ? 'reachable now' : 'verified before, not reachable now'
                    }${point.localSessionObserved ? ', a session with this node in the latest round' : ''}`}
                    opacity={selectedId === null || selectedId === point.peerId ? 1 : 0.35}
                    onMouseEnter={() => setSelectedId(point.peerId)}
                    onFocus={() => setSelectedId(point.peerId)}
                    className="cursor-pointer outline-none"
                  >
                    <circle cx={x} cy={y} r="5.5" fill="transparent" />
                    {point.localSessionObserved && (
                      <circle
                        data-ring
                        cx={x}
                        cy={y}
                        r={RING_R}
                        fill="none"
                        stroke="#dee2ec"
                        strokeWidth="1.2"
                      />
                    )}
                    {point.reachable ? (
                      <circle
                        data-dot="reachable"
                        cx={x}
                        cy={y}
                        r={DOT_R}
                        fill={DATA_COLOR}
                        fillOpacity={point.localSessionObserved ? 0.9 : 0.55}
                      />
                    ) : (
                      <circle
                        data-dot="verified-before"
                        cx={x}
                        cy={y}
                        r="3.3"
                        fill="#0c0e15"
                        stroke="#dee2ec"
                        strokeWidth="0.9"
                      />
                    )}
                  </g>
                ))}
              </svg>
              {neverVerified > 0 && (
                <div className="flex flex-col gap-1.5">
                  <span className="text-text-dim text-[12px]">
                    {neverVerified.toLocaleString()} more peers were only heard of: never verified,
                    so their place is unknown
                  </span>
                  <svg
                    role="img"
                    aria-label={`${neverVerified} peers only heard of`}
                    width={width}
                    height={12 + Math.floor((neverVerified - 1) / perRow) * 9}
                    className="block"
                  >
                    {Array.from({ length: neverVerified }, (_, k) => (
                      <circle
                        key={k}
                        data-unlocated
                        cx={6 + (k % perRow) * ((width - 12) / Math.max(1, perRow - 1))}
                        cy={6 + Math.floor(k / perRow) * 9}
                        r="1.8"
                        fill="#4b5266"
                      />
                    ))}
                  </svg>
                </div>
              )}
            </>
          )}
        </div>
        {ready && (
          <aside
            aria-label="Selected peer evidence"
            aria-live="polite"
            className="border-base-border-subtle min-w-0 self-start rounded border px-3.5 py-3 text-[12px] lg:sticky lg:top-4"
          >
            {selected ? (
              <PeerReadout point={selected} roundId={ready.roundId} />
            ) : (
              <Legend
                reachable={reachable}
                before={ready.points.length - reachable}
                ringed={ringed}
                neverVerified={neverVerified}
              />
            )}
          </aside>
        )}
      </div>
      {ready && (
        <figcaption className="text-text-dim mt-3 text-[12px] leading-relaxed">
          {ready.located.toLocaleString()} of {ready.verifiedRetained.toLocaleString()} retained
          verified peers have approximate GeoIP coordinates;{' '}
          {ready.locationUnavailable.toLocaleString()} locations are unavailable. Dots with close
          coordinates are displaced to keep them selectable. There is no centre: every node keeps a
          handful of connections of its own, and that mesh cannot be observed from outside, so no
          links are drawn.
        </figcaption>
      )}
    </figure>
  );
}
