'use client';

import { Fragment, type RefObject } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type NetworkMap } from '@/lib/api';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout, type PinnedReadout } from '@/hooks/usePinnedReadout';
import { ReadoutRow } from '@/components/ui/readout-row';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatDurationMs, formatUtc8Date, formatUtc8Time } from '@/lib/utils';

type Point = NetworkMap['points'][number];

const DOT_R = 3.6;
const RING_R = 6.2;
/** Hierarchy v3 (R13): the band above the clusters that holds the map's name, inside the plot. */
const NAME_BAND = 24;
/** The map's own floor: with its name band the picture is at least 280 px tall on a phone. */
const MIN_MAP_HEIGHT = 256;

/** Dot spacing inside a country cluster, and the map's latitude band (the Network board). */
const SPACING = 9.2;
const LAT_TOP = 72;
const LAT_BOTTOM = -48;
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
/**
 * Below this map width a country with a single verified node is not labelled on the map: its
 * "XX 1" labels crowd the dense regions (390 px: CN, TW, MO, IN, HK, SG, ID ran together). Those
 * countries are named in a line under the map instead, so every country stays readable.
 */
export const NARROW_MAP_WIDTH = 480;

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
 * displacement only: the readout keeps the source coordinates. Deterministic. On a narrow map
 * the single-node countries are `listed` for the line under the map instead of labelled on it.
 */
export function layoutCountryClusters(
  points: Point[],
  width: number,
  height: number
): {
  dots: Array<{ point: Point; x: number; y: number }>;
  labels: CountryLabel[];
  listed: { country: string; count: number }[];
} {
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
  const labelled = (country: string) =>
    width >= NARROW_MAP_WIDTH || byCountry.get(country)!.length >= 2;
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
  /** A cluster's footprint: its dots plus the label under it, when it has one. */
  const extent = (country: string) => {
    const r = radius(byCountry.get(country)!.length);
    if (!labelled(country)) return { r, half: r, below: r };
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
  const listed: { country: string; count: number }[] = [];
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
    if (labelled(country))
      labels.push({ country, count: peers.length, x: cx, y: cy + radius(peers.length) + 11 });
    else listed.push({ country, count: peers.length });
  }
  dots.sort((a, b) => a.point.peerId.localeCompare(b.point.peerId));
  return { dots, labels, listed };
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

/** The map's counts, one derivation for the key, the reading row and the notes. */
export function mapCounts(map: NetworkMap): {
  reachable: number;
  before: number;
  ringed: number;
  countries: number;
} {
  const reachable = map.points.filter((point) => point.reachable).length;
  return {
    reachable,
    before: map.points.length - reachable,
    ringed: map.points.filter((point) => point.localSessionObserved).length,
    countries: new Set(map.points.map((point) => point.country)).size,
  };
}

/**
 * The map's key, drawn (hierarchy v3: a graphic's names live in the graphic): one glyph and its
 * words per row. It stands in the panel until a node is named there.
 */
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
  const rows: [React.ReactNode, string][] = [
    [<circle key="g" cx="7" cy="0" r={DOT_R} fill={DATA_COLOR} />, `reachable now · ${reachable}`],
    [
      <circle key="g" cx="7" cy="0" r="3.3" fill="#0c0e15" stroke="#dee2ec" />,
      `verified before, not now · ${before}`,
    ],
    [
      <g key="g">
        <circle cx="7" cy="0" r="6" fill="none" stroke="#dee2ec" strokeWidth="1.2" />
        <circle cx="7" cy="0" r={DOT_R} fill={DATA_COLOR} />
      </g>,
      `this node's ${ringed} session peers`,
    ],
    [
      <circle key="g" cx="7" cy="0" r="1.8" fill="#4b5266" />,
      `heard of, never verified · ${neverVerified}`,
    ],
  ];
  const ROW = 20;
  return (
    <svg
      role="img"
      aria-label={`Key: reachable now ${reachable}, verified before, not now ${before}, this node's ${ringed} session peers, heard of, never verified ${neverVerified}`}
      width="100%"
      height={rows.length * ROW}
      className="block"
    >
      {rows.map(([glyph, words], i) => (
        <g key={words} transform={`translate(0 ${i * ROW + ROW / 2})`}>
          {glyph}
          <text x="22" y="4" fontSize="12" fill="#a0a8be">
            {words}
          </text>
        </g>
      ))}
    </svg>
  );
}

/**
 * Round 3 (contracts R-C · R-13, CH-57): this node's session with the peer in the latest round,
 * as of when the crawler read it (`observedAt`, the crawler's seconds clock), never "now".
 */
function sessionWords(point: Point): string {
  const session = point.localSession;
  if ((session !== undefined) !== point.localSessionObserved)
    throw new Error(
      `peer ${point.peerId}: localSession ${session ? 'present' : 'absent'} but localSessionObserved is ${point.localSessionObserved}`
    );
  if (!session) return 'no session in the latest round';
  const ping =
    session.lastPingDurationMs === undefined ? '' : ` · ping ${session.lastPingDurationMs} ms`;
  const dial =
    session.initiator === 'observerInitiated' ? 'this node dialled it' : 'it dialled this node';
  return `connected for ${formatDurationMs(session.connectedDurationMs)}${ping} · ${dial} (as of ${formatUtc8Time(session.observedAt * 1000)})`;
}

/** A node's identity in one line, for the readout under the map: who, where, state, client. */
function peerLine(point: Point): string {
  const who = point.alias
    ? `${point.alias.address} · peer ${shortPeerId(point.peerId)}`
    : `peer ${shortPeerId(point.peerId)}`;
  const state = point.reachable ? 'reachable now' : 'verified before, not reachable now';
  return `${who} · ${point.country} · ${state} · ${point.clientVersion}`;
}

function PeerReadout({ point, roundId }: { point: Point; roundId: number | null }) {
  const rows: [string, string, string?][] = [
    // With an alias the panel is titled by it, so the peer id is a row (R-13).
    ...(point.alias ? [['peer', shortPeerId(point.peerId)] as [string, string]] : []),
    [
      'state',
      point.reachable ? 'reachable now' : 'verified before, not reachable now',
      point.reachable ? 'text-jade' : undefined,
    ],
    ['to this node', sessionWords(point), point.localSession ? 'text-jade' : undefined],
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
      <p className="text-text-bright text-body [overflow-wrap:anywhere]">
        {point.alias ? point.alias.address : `peer ${shortPeerId(point.peerId)}`}
      </p>
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

/** The map's shared state: its data, its measured size and the node a pointer or a tap names. */
export interface NetworkMapState {
  neverVerified: number;
  readout: PinnedReadout<string>;
  ref: RefObject<HTMLDivElement | null>;
  width: number;
  height: number;
  isLoading: boolean;
  error: unknown;
  ready: NetworkMap | null;
  placed: ReturnType<typeof layoutCountryClusters> | null;
  selected: Point | null;
}

/**
 * The map's state, for a page that places the picture and the lines under it apart (the hero's
 * section holds the picture; its reading follows the section). A refetched map without the pinned
 * (or hovered) node lets it go: a reading names a drawn node.
 */
export function useNetworkMap(neverVerified: number): NetworkMapState {
  // A named node stays named until the pointer leaves the map (sticky), as before R6.
  const readout = usePinnedReadout<string>(String, { sticky: true });
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
  const height = Math.max(MIN_MAP_HEIGHT, Math.round(width * 0.49));
  const ready = !isLoading && !error && data ? data : null;
  const placed = ready ? layoutCountryClusters(ready.points, width, height) : null;
  const selectedId = readout.active;
  if (selectedId !== null && ready && !ready.points.some((point) => point.peerId === selectedId))
    readout.clear();
  const selected = ready?.points.find((point) => point.peerId === selectedId) ?? null;
  return { neverVerified, readout, ref, width, height, isLoading, error, ready, placed, selected };
}

/**
 * Where the verified nodes are (Network board V70): every verified peer a dot at its GeoIP place,
 * reachable solid, verified-before hollow, this node's session peers ringed wherever they are. No
 * link is drawn and nothing is centred (user correction). Beside it, the panel that names a hovered
 * or focused node; a tap pins it there until a second tap on it or Escape (R6).
 */
export function NetworkMapPicture({ map }: { map: NetworkMapState }) {
  const { readout, ref, width, height, isLoading, error, ready, placed, selected } = map;
  const selectedId = readout.active;
  const layout = placed?.dots ?? [];
  const countryLabels = placed?.labels ?? [];
  const counts = ready ? mapCounts(ready) : null;
  return (
    <div className="min-w-0">
      <div className="grid min-w-0 gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
        {/* Measured from mount, so the map is drawn 1:1 in its own column at every width. */}
        <div ref={ref} className="flex min-w-0 flex-col gap-3">
          {isLoading ? (
            <p className="text-text-dim text-small">Loading crawler locations…</p>
          ) : !ready ? (
            <p role="alert" className="text-negative text-small">
              Crawler locations unavailable: {String(error)}
            </p>
          ) : (
            <svg
              width={width}
              height={height + NAME_BAND}
              viewBox={`0 0 ${width} ${height + NAME_BAND}`}
              role="img"
              aria-label="Approximate GeoIP locations of retained verified CKB peers"
              className="block"
              onMouseLeave={() => readout.hover(null)}
            >
              {[1, 2, 3, 4, 5, 6, 7].map((k) => (
                <line
                  key={'lon' + k}
                  x1={(width * k) / 8}
                  x2={(width * k) / 8}
                  y1={NAME_BAND}
                  y2={height + NAME_BAND}
                  stroke="#121620"
                />
              ))}
              {[1, 2, 3, 4, 5].map((k) => (
                <line
                  key={'lat' + k}
                  x1="0"
                  x2={width}
                  y1={NAME_BAND + (height * k) / 6}
                  y2={NAME_BAND + (height * k) / 6}
                  stroke={k === 3 ? '#1a1f30' : '#121620'}
                />
              ))}
              <text data-role="chart-name" x="8" y="14" fontSize="11" fill="#7c849c">
                verified nodes · approximate GeoIP place
              </text>
              {countryLabels.map(({ country, count, x, y }) => (
                <text
                  key={country}
                  data-country-label
                  x={x}
                  y={y + NAME_BAND}
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
              {layout.map(({ point, x, y: mapY }) => {
                const y = mapY + NAME_BAND;
                return (
                  <g
                    key={point.peerId}
                    data-peer={point.peerId}
                    {...readout.bind(point.peerId)}
                    aria-label={`${point.country} peer ${shortPeerId(point.peerId)}, ${
                      point.reachable ? 'reachable now' : 'verified before, not reachable now'
                    }${point.localSessionObserved ? ', a session with this node in the latest round' : ''}`}
                    opacity={selectedId === null || selectedId === point.peerId ? 1 : 0.35}
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
                );
              })}
            </svg>
          )}
        </div>
        {ready && (
          <aside
            aria-label="Selected peer evidence"
            aria-live="polite"
            className="border-base-border-subtle text-small min-w-0 self-start rounded border px-3.5 py-3 lg:sticky lg:top-4"
          >
            {selected ? (
              <PeerReadout point={selected} roundId={ready.roundId} />
            ) : (
              <Legend
                reachable={counts!.reachable}
                before={counts!.before}
                ringed={counts!.ringed}
                neverVerified={map.neverVerified}
              />
            )}
          </aside>
        )}
      </div>
    </div>
  );
}

/**
 * The lines under the map: the reading of the named node, the single-node countries a phone map
 * does not label, the never-verified peers as a strip (data, so it stays with the picture), and
 * the map's notes.
 */
export function NetworkMapFootnotes({ map }: { map: NetworkMapState }) {
  const { readout, width, ready, placed, selected, neverVerified } = map;
  if (!ready) return null;
  const listedCountries = placed?.listed ?? [];
  const perRow = Math.max(8, Math.floor(width / 14));
  return (
    <div className="mt-3 flex min-w-0 flex-col gap-3">
      <ReadoutRow
        label="Node readout"
        pinned={readout.showingPin}
        idle={`${ready.located.toLocaleString()} verified nodes in ${mapCounts(ready).countries} countries`}
      >
        {selected ? peerLine(selected) : null}
      </ReadoutRow>
      {listedCountries.length > 0 && (
        <p
          aria-label="Countries with one verified node"
          className="text-text-dim text-small leading-relaxed"
        >
          One verified node each, not labelled on the map:{' '}
          {listedCountries.map(({ country, count }, i) => (
            <Fragment key={country}>
              {i > 0 && ' · '}
              <span data-country-listed className="whitespace-nowrap">
                <span className="text-text">{country}</span> {count}
              </span>
            </Fragment>
          ))}
        </p>
      )}
      {neverVerified > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-text-dim text-small">
            {neverVerified.toLocaleString()} more peers were only heard of: never verified, so their
            place is unknown
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
      {/* One caption (R3); the coverage, displacement and no-centre notes are tier 2's. */}
      <p data-role="caption" className="text-text text-body max-w-[640px]">
        Every node this crawler verified, at its approximate GeoIP place; no links are drawn,
        because no one can see them from outside.
      </p>
    </div>
  );
}

/**
 * Tier 2: what the map and its key leave out of tier 1 — the counts behind it, how much of it has a
 * place, why close dots move, and why no link is drawn. Shares the map's query (one fetch).
 */
export function NetworkMapNotes() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['network', 'map'],
    queryFn: api.getNetworkMap,
    refetchInterval: 30000,
  });
  if (isLoading) return null;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-small">
        Crawler locations unavailable: {String(error)}
      </p>
    );
  const counts = mapCounts(data);
  return (
    <p className="text-text-dim text-body">
      {data.located.toLocaleString()} verified nodes in {counts.countries} countries · ringed: the{' '}
      {counts.ringed} this node had a session with in the latest round.{' '}
      {data.located.toLocaleString()} of {data.verifiedRetained.toLocaleString()} retained verified
      peers have approximate GeoIP coordinates; {data.locationUnavailable.toLocaleString()}{' '}
      locations are unavailable. Dots with close coordinates are displaced to keep them selectable.
      There is no centre: every node keeps a handful of connections of its own, and that mesh cannot
      be observed from outside, so no links are drawn. Each node keeps its own handful of
      connections to others. Those links cannot be seen from outside, so none are drawn: not even
      this node&apos;s, which would make it look like a centre.
    </p>
  );
}

/** The map and the lines under it, together (a page without a hero to split them across). */
export function NetworkMapView({ neverVerified }: { neverVerified: number }) {
  const map = useNetworkMap(neverVerified);
  return (
    <div className="min-w-0">
      <NetworkMapPicture map={map} />
      <NetworkMapFootnotes map={map} />
    </div>
  );
}
