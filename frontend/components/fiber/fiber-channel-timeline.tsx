'use client';

import type { FiberChannelDetail } from '@/lib/api';
import { FIBER_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { epochDelayWords, fiberDuration } from '@/lib/fiber-format';
import { formatUtc8Date } from '@/lib/utils';
import { useElementWidth } from '@/hooks/useElementWidth';
import { SVG_TEXT, lineAscentPx } from '@/lib/chart-type';
import { descentPx, tightLinePx } from '@/lib/type-metrics';

/** The board's drawing width, used until the box is measured. */
const DESIGN_WIDTH = 1232;
const L = 40;
/*
 * The timeline's words stand in tight lines of their roles (readability plan 2026-11-18, lane F3):
 * a point's name (value) and under it its block and date (label), one line or, below 700 px, two.
 */
/** The line, its open stroke 4 px wide, its points 6 px circles under a 1.5 px stroke. */
const LINE_Y = 30;
const OPEN_STROKE = 4;
const POINT_R = 6;
const POINT_STROKE = 1.5;
/**
 * The open segment's words stand a pixel over the points' circles: on a phone they are wider than
 * the segment and cross its ends.
 */
const SEGMENT_Y = LINE_Y - POINT_R - POINT_STROKE / 2 - 1 - descentPx(SVG_TEXT.label);
/** The wait's words stand under the line, a pixel under a point's circle. */
const WAIT_Y = LINE_Y + POINT_R + 1 + lineAscentPx(SVG_TEXT.note);
/** A point's words under the line start 14 px under it, clear of its circle and the wait. */
const BELOW_TOP = LINE_Y + 14;
/** A point's name over the line: on the svg's top line (the waiting label's), two lines higher below 700 px. */
const ABOVE_Y = 0;
/** The step from a point's name to its block line, and from that line to the next. */
const NAME_STEP = descentPx(SVG_TEXT.value) + lineAscentPx(SVG_TEXT.label);
const LINE_STEP = tightLinePx(SVG_TEXT.label);
/** The svg keeps 2.25 px over its highest word and under its lowest. */
const EDGE = 2.25;

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
  // Drawn 1:1 in its box; below 700 px each point's block and date go on two lines (390 pass).
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const W = measured ?? DESIGN_WIDTH;
  const narrow = W < 700;
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
      // A force close starts the wait, and above the line at the wait's right end stands its own
      // label ("settled", or "not settled · wait …" while it runs): the close goes under the line,
      // settled or not (readability 2026-11-18: above it, "force-closed" met the wait's words).
      place: forced ? 'below-end' : 'above-end',
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
  const right = waiting ? W - Math.min(200, W * 0.3) : W - L;
  const t0 = points[0].t;
  const t1 = Math.max(points[points.length - 1].t, t0 + 1);
  const X = (t: number) => L + ((t - t0) / (t1 - t0)) * (right - L);
  const [open, second, third] = points;
  const aboveY = narrow ? ABOVE_Y - 2 * LINE_STEP : ABOVE_Y;
  const belowY = BELOW_TOP + lineAscentPx(SVG_TEXT.value);
  // the lowest word: a point's block (and its date on a narrow svg) under its name
  const foot = belowY + NAME_STEP + (narrow ? LINE_STEP : 0) + descentPx(SVG_TEXT.label);
  const top = Math.floor(aboveY - lineAscentPx(SVG_TEXT.value) - EDGE);
  const bottom = Math.ceil(foot + EDGE);
  const aria =
    channel.state === 'open'
      ? `Opened, open for ${lifetime} at the indexed tip`
      : channel.state === 'cooperativelyClosed'
        ? `Opened, closed together ${lifetime} later`
        : channel.state === 'forceClosed'
          ? `Opened, force-closed ${lifetime} later, not settled yet`
          : `Opened, force-closed ${lifetime} later, settled after a wait of ${delay}`;
  return (
    <div ref={box} className="min-w-0">
      <svg
        viewBox={`0 ${top} ${W} ${bottom - top}`}
        role="img"
        aria-label={aria}
        className="block h-auto w-full"
      >
        <line
          data-segment="open"
          x1={X(open.t)}
          x2={X(second.t)}
          y1={LINE_Y}
          y2={LINE_Y}
          stroke={FIBER_COLORS.open}
          strokeWidth={OPEN_STROKE}
          strokeOpacity=".6"
        />
        <text
          x={(X(open.t) + X(second.t)) / 2}
          y={SEGMENT_Y}
          textAnchor="middle"
          fontSize={SVG_TEXT.label}
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
              y1={LINE_Y}
              y2={LINE_Y}
              stroke={FIBER_COLORS.forced}
              strokeWidth="1.5"
              strokeDasharray="3 3"
            />
            <text
              x={narrow ? X(second.t) + 10 : (X(second.t) + X(third.t)) / 2}
              y={WAIT_Y}
              textAnchor={narrow ? 'start' : 'middle'}
              fontSize={SVG_TEXT.note}
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
              y1={LINE_Y}
              y2={LINE_Y}
              stroke={FIBER_COLORS.forced}
              strokeWidth="1.5"
              strokeDasharray="3 3"
            />
            <text
              x={W - L}
              y={ABOVE_Y}
              textAnchor="end"
              fontSize={SVG_TEXT.value}
              fill={FIBER_COLORS.count}
            >
              not settled · wait {delay}
            </text>
          </>
        )}
        {points.map((point) => {
          const x = X(point.t);
          const [anchor, dx, y] =
            point.place === 'below-start'
              ? (['start', -6, belowY] as const)
              : point.place === 'below-end'
                ? (['end', -10, belowY] as const)
                : (['end', 6, aboveY] as const);
          return (
            <g key={point.key}>
              <circle
                data-point={point.key}
                cx={x}
                cy={LINE_Y}
                r={POINT_R}
                fill={FIBER_COLORS.halo}
                stroke={FIBER_COLORS.count}
                strokeWidth={POINT_STROKE}
                strokeDasharray={point.hollow ? '2 2' : undefined}
              />
              <text
                x={x + dx}
                y={y}
                textAnchor={anchor}
                fontSize={SVG_TEXT.value}
                fill={FIBER_COLORS.count}
              >
                {point.label}
              </text>
              {narrow ? (
                <>
                  <text
                    x={x + dx}
                    y={y + NAME_STEP}
                    textAnchor={anchor}
                    fontSize={SVG_TEXT.label}
                    fill={FIBER_COLORS.note}
                  >
                    {formatBlockNumber(point.block)}
                  </text>
                  <text
                    x={x + dx}
                    y={y + NAME_STEP + LINE_STEP}
                    textAnchor={anchor}
                    fontSize={SVG_TEXT.label}
                    fill={FIBER_COLORS.note}
                  >
                    {formatUtc8Date(point.t)}
                  </text>
                </>
              ) : (
                <text
                  x={x + dx}
                  y={y + NAME_STEP}
                  textAnchor={anchor}
                  fontSize={SVG_TEXT.label}
                  fill={FIBER_COLORS.note}
                >
                  {`${formatBlockNumber(point.block)} · ${formatUtc8Date(point.t)}`}
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}
