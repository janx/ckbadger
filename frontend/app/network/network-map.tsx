'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type NetworkMap } from '@/lib/api';
import { formatUtc8Date } from '@/lib/utils';

type Point = NetworkMap['points'][number];

/** Stable visual displacement only; the source coordinates remain in the readout. */
export function layoutMapPoints(points: Point[]): Array<{ point: Point; x: number; y: number }> {
  const occupied = new Set<string>();
  const columns = 90;
  const rows = 45;
  return [...points]
    .sort((a, b) => a.peerId.localeCompare(b.peerId))
    .map((point) => {
      const originColumn = Math.max(
        0,
        Math.min(columns - 1, Math.round(((point.lon + 180) * 2 - 4) / 8))
      );
      const originRow = Math.max(0, Math.min(rows - 1, Math.round(((90 - point.lat) * 2 - 4) / 8)));
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
            return { point, x: 4 + column * 8, y: 4 + row * 8 };
          }
        }
      }
      throw new Error(
        `Network map cannot place peer ${point.peerId}: more than ${columns * rows} located peers`
      );
    });
}

/** Approximate GeoIP locations of retained verified peers; no p2p links are inferred. */
export function NetworkMapView() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const { data, isLoading, error } = useQuery({
    queryKey: ['network', 'map'],
    queryFn: api.getNetworkMap,
    refetchInterval: 30000,
  });
  if (isLoading) return <p className="text-text-dim text-[12px]">Loading crawler locations…</p>;
  if (error || !data)
    return (
      <p role="alert" className="text-negative text-[12px]">
        Crawler locations unavailable: {String(error)}
      </p>
    );
  const layout = layoutMapPoints(data.points);
  const selected = data.points.find((point) => point.peerId === selectedId) ?? data.points[0];
  const countries = new Map<string, number>();
  for (const point of data.points)
    countries.set(point.country, (countries.get(point.country) ?? 0) + 1);
  const countryCounts = [...countries.entries()].sort(
    (a, b) => b[1] - a[1] || a[0].localeCompare(b[0])
  );
  return (
    <figure className="min-w-0">
      <div className="grid min-w-0 gap-5 lg:grid-cols-[minmax(0,1fr)_16rem]">
        <svg
          viewBox="0 0 720 360"
          role="img"
          aria-label="Approximate GeoIP locations of retained verified CKB peers"
          className="border-base-border-subtle bg-base-surface block h-auto w-full border"
        >
          {[90, 180, 270, 360, 450, 540, 630].map((x) => (
            <line key={'lon' + x} x1={x} x2={x} y1="0" y2="360" stroke="#222840" />
          ))}
          {[60, 120, 180, 240, 300].map((y) => (
            <line key={'lat' + y} x1="0" x2="720" y1={y} y2={y} stroke="#222840" />
          ))}
          {layout.map(({ point, x, y }) => (
            <circle
              key={point.peerId}
              cx={x}
              cy={y}
              r={selected?.peerId === point.peerId ? 5 : 3.5}
              fill={point.reachable ? '#2edba3' : '#a0a8be'}
              stroke="#0c0e15"
              strokeWidth="1"
              tabIndex={0}
              onMouseEnter={() => setSelectedId(point.peerId)}
              onFocus={() => setSelectedId(point.peerId)}
              aria-label={
                point.country +
                ' peer ' +
                point.peerId +
                (point.reachable
                  ? ', reachable in last round'
                  : ', verified before but unavailable in last round')
              }
            >
              <title>
                {point.country} · {point.peerId}
              </title>
            </circle>
          ))}
        </svg>
        <aside
          aria-label="Selected peer evidence"
          className="border-base-border-subtle min-w-0 border-t pt-3 text-[12px]"
        >
          {selected ? (
            <>
              <p className="text-text-bright break-all">{selected.peerId}</p>
              <dl className="mt-3 grid grid-cols-[6rem_minmax(0,1fr)] gap-y-2">
                <dt className="text-text-dim">State</dt>
                <dd>
                  {selected.reachable
                    ? 'same-network reachable'
                    : 'verified earlier; unavailable last round'}
                </dd>
                <dt className="text-text-dim">Client</dt>
                <dd className="break-words">{selected.clientVersion}</dd>
                <dt className="text-text-dim">Country</dt>
                <dd>{selected.country}</dd>
                <dt className="text-text-dim">Network</dt>
                <dd className="break-words">{selected.asn ?? 'Unknown'}</dd>
                <dt className="text-text-dim">GeoIP</dt>
                <dd>
                  {selected.lat.toFixed(2)}°, {selected.lon.toFixed(2)}°
                </dd>
                <dt className="text-text-dim">First seen</dt>
                <dd>{formatUtc8Date(selected.firstSeen * 1000)}</dd>
                <dt className="text-text-dim">Advertisers</dt>
                <dd>
                  {selected.advertiserCount.toLocaleString()} retained in round #{data.roundId}
                </dd>
                <dt className="text-text-dim">Local session</dt>
                <dd>
                  {selected.localSessionObserved
                    ? 'observed in last completed round'
                    : 'no observation retained for last round'}
                </dd>
              </dl>
            </>
          ) : (
            <p className="text-text-dim">No located peers.</p>
          )}
        </aside>
      </div>
      <ul
        aria-label="Located peers by country"
        className="mt-4 flex flex-wrap gap-x-5 gap-y-1 text-[11px]"
      >
        {countryCounts.map(([country, count]) => (
          <li key={country}>
            <span className="text-text-bright">{country}</span>{' '}
            <span className="text-text-dim">{count}</span>
          </li>
        ))}
      </ul>
      <figcaption className="text-text-dim mt-3 text-[12px] leading-relaxed">
        {data.located.toLocaleString()} of {data.verifiedRetained.toLocaleString()} retained
        verified peers have approximate GeoIP coordinates;{' '}
        {data.locationUnavailable.toLocaleString()} locations are unavailable. Dots with close
        coordinates are displaced to keep them selectable; the readout shows the source coordinates.
        Peer-to-peer connections cannot be observed from outside, so no connection lines are drawn.
      </figcaption>
    </figure>
  );
}
