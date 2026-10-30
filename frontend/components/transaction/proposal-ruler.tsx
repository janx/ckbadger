import { formatCount } from '@/lib/format-count';

const X0 = 8;
const X1 = 248;
const Y = 11;

/**
 * "Proposed, then committed" as a block ruler (Main.dc.html): a gold dot at the proposing block,
 * the commit window (gold, faint) from `close` to `far` blocks after it, ticks per block, and a
 * jade square where the transaction was committed. Gold here means "proposed", its one other use.
 */
export function ProposalRuler({
  proposedIn,
  committedIn,
  window,
}: {
  proposedIn: number;
  committedIn: number;
  window: { close: number; far: number };
}) {
  const distance = committedIn - proposedIn;
  if (
    !Number.isSafeInteger(window.close) ||
    !Number.isSafeInteger(window.far) ||
    window.close < 1 ||
    window.far < window.close
  )
    throw new Error(`Invalid commit window ${window.close}–${window.far}`);
  if (distance < window.close || distance > window.far)
    throw new Error(
      `Block ${committedIn} committed ${distance} blocks after its proposal, outside the window ${window.close}–${window.far}`
    );
  const step = (X1 - X0) / window.far;
  const at = (offset: number) => X0 + offset * step;
  const tail = (n: number) => `…${String(n).slice(-3)}`;
  const n = (v: number) => v.toLocaleString('en-US');
  return (
    <svg
      viewBox="0 0 262 30"
      role="img"
      aria-label={`Proposed in block ${n(proposedIn)}, committed ${formatCount(distance, 'block')} later in ${n(committedIn)}; the commit window is ${window.close} to ${window.far} blocks`}
      className="block h-auto w-full max-w-[262px]"
    >
      <path d={`M${X0} ${Y}H${X1}`} className="stroke-base-border" />
      <path
        data-window
        d={`M${at(window.close)} ${Y}H${X1}`}
        className="stroke-gold"
        strokeOpacity={0.4}
        strokeWidth={2}
      />
      {Array.from({ length: window.far }, (_, i) => (
        <path key={i} data-tick d={`M${at(i + 1)} 8v6`} className="stroke-base-border" />
      ))}
      <circle data-proposal cx={X0} cy={Y} r={3.6} className="fill-gold" />
      <rect data-commit x={at(distance) - 4} y={Y - 4} width={8} height={8} className="fill-jade" />
      <g className="fill-text-dim" fontSize={10} fontFamily="inherit">
        <text x={2} y={28}>
          {tail(proposedIn)}
        </text>
        <text x={at(distance)} y={28} textAnchor="middle">
          {tail(committedIn)}
        </text>
        <text x={X1} y={28} textAnchor="end">
          +{window.far}
        </text>
      </g>
    </svg>
  );
}
