'use client';

import type { FiberChannelDetail } from '@/lib/api';
import { FIBER_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { epochDelayWords, fiberDuration } from '@/lib/fiber-format';
import { formatUtc8Date } from '@/lib/utils';

const W = 1232;
const L = 40;

interface Point {
  key: string;
  t: number;
  block: number;
  label: string;
  /** Label placement, as on the board: under-left, under-right, or above-right. */
  place: 'below-start' | 'below-end' | 'above-end';
  hollow?: boolean;
}

function ms(value: string, what: string): number {
  if (!/^(0|[1-9]\d*)$/.test(value)) throw new Error(`Fiber timeline: invalid ${what} ${value}`);
  return Number(value);
}

function required<T>(value: T | null, what: string, channelId: string): T {
  if (value === null) throw new Error(`Fiber channel ${channelId} lacks ${what}`);
  return value;
}

/** "Opened → (force-)closed → settled" on one time axis (FiberChannel board V66). */
export function FiberChannelTimeline({ channel }: { channel: FiberChannelDetail }) {
  const id = channel.channelId;
  const lifetime = fiberDuration(
    required(channel.observedLifetimeMs, 'an observed lifetime', id),
    `${id} lifetime`
  );
  const forced = channel.state === 'forceClosed' || channel.state === 'settled';
  const delay = forced
    ? epochDelayWords(required(channel.delayEpochDecoded, 'a decoded commitment delay', id))
    : null;
  const tOpen = ms(channel.openTimestamp, 'open time');
  const points: Point[] = [
    { key: 'open', t: tOpen, block: channel.openBlock, label: 'opened', place: 'below-start' },
  ];
  if (channel.state === 'open') {
    points.push({
      key: 'tip',
      t: tOpen + ms(channel.observedLifetimeMs!, 'lifetime'),
      block: required(channel.lifetimeAnchorBlock, 'a lifetime anchor block', id),
      label: 'indexed tip',
      place: 'above-end',
      hollow: true,
    });
  } else {
    points.push({
      key: 'close',
      t: ms(required(channel.closeTimestamp, 'a close time', id), 'close time'),
      block: required(channel.closeBlock, 'a close block', id),
      label: forced ? 'force-closed' : 'closed together',
      place: channel.state === 'settled' ? 'below-end' : 'above-end',
    });
  }
  if (channel.state === 'settled')
    points.push({
      key: 'settle',
      t: ms(required(channel.settlementTimestamp, 'a settlement time', id), 'settlement time'),
      block: required(channel.settlementBlock, 'a settlement block', id),
      label: 'settled',
      place: 'above-end',
    });
  const waiting = channel.state === 'forceClosed';
  const right = waiting ? W - 200 : W - L;
  const t0 = points[0].t;
  const t1 = Math.max(points[points.length - 1].t, t0 + 1);
  const X = (t: number) => L + ((t - t0) / (t1 - t0)) * (right - L);
  const [open, second, third] = points;
  const aria =
    channel.state === 'open'
      ? `Opened, open for ${lifetime} at the indexed tip`
      : channel.state === 'cooperativelyClosed'
        ? `Opened, closed together ${lifetime} later`
        : channel.state === 'forceClosed'
          ? `Opened, force-closed ${lifetime} later, not settled yet`
          : `Opened, force-closed ${lifetime} later, settled after a wait of ${delay}`;
  return (
    <svg viewBox={`0 -14 ${W} 92`} role="img" aria-label={aria} className="block h-auto w-full">
      <line
        data-segment="open"
        x1={X(open.t)}
        x2={X(second.t)}
        y1="30"
        y2="30"
        stroke={FIBER_COLORS.open}
        strokeWidth="4"
        strokeOpacity=".6"
      />
      <text
        x={(X(open.t) + X(second.t)) / 2}
        y="24"
        textAnchor="middle"
        fontSize="11"
        fill={FIBER_COLORS.open}
      >
        open {lifetime}
        {channel.state === 'open' ? ' so far' : ''} · payments off chain
      </text>
      {third && (
        <>
          <line
            data-segment="wait"
            x1={X(second.t)}
            x2={X(third.t)}
            y1="30"
            y2="30"
            stroke={FIBER_COLORS.forced}
            strokeWidth="1.5"
            strokeDasharray="3 3"
          />
          <text
            x={(X(second.t) + X(third.t)) / 2}
            y="48"
            textAnchor="middle"
            fontSize="10.5"
            fill={FIBER_COLORS.history}
          >
            {delay}
          </text>
        </>
      )}
      {waiting && (
        <>
          <line
            data-segment="wait"
            x1={X(second.t)}
            x2={W - L}
            y1="30"
            y2="30"
            stroke={FIBER_COLORS.forced}
            strokeWidth="1.5"
            strokeDasharray="3 3"
          />
          <text x={W - L} y="0" textAnchor="end" fontSize="11.5" fill={FIBER_COLORS.count}>
            not settled · wait {delay}
          </text>
        </>
      )}
      {points.map((point) => {
        const x = X(point.t);
        const [anchor, dx, y] =
          point.place === 'below-start'
            ? (['start', -6, 56] as const)
            : point.place === 'below-end'
              ? (['end', -10, 56] as const)
              : (['end', 6, 0] as const);
        return (
          <g key={point.key}>
            <circle
              data-point={point.key}
              cx={x}
              cy="30"
              r="6"
              fill={FIBER_COLORS.halo}
              stroke={FIBER_COLORS.count}
              strokeWidth="1.5"
              strokeDasharray={point.hollow ? '2 2' : undefined}
            />
            <text x={x + dx} y={y} textAnchor={anchor} fontSize="11.5" fill={FIBER_COLORS.count}>
              {point.label}
            </text>
            <text x={x + dx} y={y + 15} textAnchor={anchor} fontSize="11" fill={FIBER_COLORS.note}>
              {`${formatBlockNumber(point.block)} · ${formatUtc8Date(point.t)}`}
            </text>
          </g>
        );
      })}
    </svg>
  );
}
