'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type NetworkMap } from '@/lib/api';
import { useElementWidth } from '@/hooks/useElementWidth';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatUtc8Date } from '@/lib/utils';

type Point = NetworkMap['points'][number];

/** Map grid cell, in px: one dot per cell, so neighbours never overlap. */
const CELL = 8;
const DOT_R = 3.6;
const RING_R = 6.2;

/**
 * Stable visual displacement only; the source coordinates remain in the readout. `columns` and
 * `rows` size the grid (default 90 × 45, a 720 × 360 map); each dot takes the free cell nearest
 * its GeoIP position.
 */
export function layoutMapPoints(
  points: Point[],
  columns = 90,
  rows = 45
): Array<{ point: Point; x: number; y: number }> {
  const occupied = new Set<string>();
  return [...points]
    .sort((a, b) => a.peerId.localeCompare(b.peerId))
    .map((point) => {
      const px = ((point.lon + 180) / 360) * columns * CELL;
      const py = ((90 - point.lat) / 180) * rows * CELL;
      const originColumn = Math.max(0, Math.min(columns - 1, Math.round((px - 4) / CELL)));
      const originRow = Math.max(0, Math.min(rows - 1, Math.round((py - 4) / CELL)));
      for (let radius = 0; radius < Math.max(columns, rows); radius++) {
        for (let row = originRow - radius; row <= originRow + radius; row++) {
          for (let column = originColumn - radius; column <= originColumn + radius; column++) {
            if (
              Math.max(Math.abs(column - originColumn), Math.abs(row - originRow)) !== radius ||
              column < 0 ||
              column >= columns ||
              row < 0 ||
              row >= rows
            )
              continue;
            const key = column + ':' + row;
            if (occupied.has(key)) continue;
            occupied.add(key);
            return { point, x: 4 + column * CELL, y: 4 + row * CELL };
          }
        }
      }
      throw new Error(
        `Network map cannot place peer ${point.peerId}: more than ${columns * rows} located peers`
      );
    });
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
  const columns = Math.max(24, Math.floor(width / CELL));
  const rows = Math.max(12, Math.round(columns / 2));
  const height = rows * CELL;
  const ready = !isLoading && !error && data ? data : null;
  const layout = ready ? layoutMapPoints(ready.points, columns, rows) : [];
  const selected = ready?.points.find((point) => point.peerId === selectedId) ?? null;
  const clusters = new Map<string, { xs: number[]; bottom: number }>();
  for (const { point, x, y } of layout) {
    const cluster = clusters.get(point.country) ?? { xs: [], bottom: 0 };
    cluster.xs.push(x);
    cluster.bottom = Math.max(cluster.bottom, y);
    clusters.set(point.country, cluster);
  }
  const reachable = ready?.points.filter((point) => point.reachable).length ?? 0;
  const ringed = ready?.points.filter((point) => point.localSessionObserved).length ?? 0;
  const perRow = Math.max(8, Math.floor(width / 14));
  return (
    <figure className="m-0 min-w-0">
      {ready && (
        <p className="text-text-dim mb-3 text-[12px]">
          {ready.located.toLocaleString()} verified nodes in {clusters.size} countries · ringed: the{' '}
          {ringed} this node had a session with in the latest round
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
                {[...clusters.entries()].map(([country, cluster]) => (
                  <text
                    key={country}
                    data-country-label
                    x={cluster.xs.reduce((sum, x) => sum + x, 0) / cluster.xs.length}
                    y={cluster.bottom + 13}
                    textAnchor="middle"
                    fontSize="10"
                    fill="#7c849c"
                    stroke="#0c0e15"
                    strokeWidth="3"
                    paintOrder="stroke"
                  >
                    {country} {cluster.xs.length}
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
