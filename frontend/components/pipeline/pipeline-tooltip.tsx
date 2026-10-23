'use client';

import { forwardRef } from 'react';
import { createPortal } from 'react-dom';
import type {
  PipelineBlock,
  PipelinePoolTx,
  PipelineProposal,
  PipelineSnapshot,
  PipelineTx,
} from '@/lib/api';
import { shadeStep, tok } from '@/lib/pipeline/encoding';
import { inWindow, tail4, windowRange } from '@/lib/pipeline/layout';
import { fmtInt } from '@/components/pipeline/svg-text';

/** What a hover explains: a header with a tag, label/value rows and an optional footnote. */
export interface TipContent {
  key: string;
  tone: string;
  title: string;
  tag: string | null;
  rows: [string, string][];
  foot: string | null;
}

export type TxInfo =
  | { kind: 'pending'; tx: PipelinePoolTx }
  | { kind: 'proposed'; tx: PipelinePoolTx; tooEarly: boolean }
  | { kind: 'committed'; tx: PipelineTx; block: PipelineBlock }
  | { kind: 'cellbase'; block: PipelineBlock };

export const fmtBytes = (b: number) =>
  b >= 1e6 ? `${(b / 1e6).toFixed(2)} MB` : b >= 1000 ? `${(b / 1000).toFixed(1)} KB` : `${b} B`;
export const fmtCycles = (c: number) =>
  c >= 1e9 ? `${(c / 1e9).toFixed(2)} B` : c >= 1e6 ? `${(c / 1e6).toFixed(1)} M` : fmtInt(c);
export const pct = (a: number, b: number) => {
  const p = (a / b) * 100;
  return p <= 0 ? '0%' : p < 0.01 ? '<0.01%' : p < 1 ? `${p.toFixed(2)}%` : `${p.toFixed(1)}%`;
};
export const shortHash = (h: string) => `${h.slice(0, 8)}…${h.slice(-6)}`;
const num = (n: number) => `#${fmtInt(n)}`;
const STEPS = ['few', 'moderate', 'many', 'very many'];
const TONE = {
  pending: tok('aqua'),
  proposed: tok('gold'),
  committed: tok('jade'),
  dim: tok('ink-dim'),
};

/** Whether the next block may commit a tx proposed at height p, in words. */
export function commitNote(p: number, tooEarly: boolean, s: PipelineSnapshot): string {
  if (tooEarly) return 'not yet: proposed in the last block';
  const left = p + s.consensus.proposalWindowFarthest - s.tip.number;
  if (left <= 0) return 'no · its window has closed';
  if (left === 1) return 'yes · last chance in the next block';
  return `yes · ${left} blocks left`;
}

const feeRate = (rate: string) => `${fmtInt(Number(rate))} sh/kB`;

export function txTip(info: TxInfo, s: PipelineSnapshot, now: number): TipContent {
  if (info.kind === 'cellbase') return cellbaseTip(info.block, s);
  const t = info.tx;
  const rows: [string, string][] = [
    ['size · area', `${fmtBytes(t.size)} · ${pct(t.size, s.consensus.maxBlockBytes)}`],
  ];
  if (info.kind === 'committed') {
    const c = info.tx;
    rows.push([
      'cycles · shade',
      c.cycles === null
        ? c.cyclesStatus === 'failed'
          ? 'calculation failed'
          : 'not computed yet'
        : `${fmtCycles(c.cycles)} · ${STEPS[shadeStep(c.cycles)]}`,
    ]);
  } else {
    rows.push([
      'cycles · shade',
      `${fmtCycles(info.tx.cycles)} · ${STEPS[shadeStep(info.tx.cycles)]}`,
    ]);
  }
  rows.push(['fee rate', feeRate(t.feeRate)], ['fee', `${fmtInt(Number(t.fee))} sh`]);
  let foot = 'Click to pin it and follow it from the pool to the block that commits it.';
  if (info.kind === 'pending') {
    rows.push(['waiting', `${((now - info.tx.arrivedAt) / 1000).toFixed(1)} s`]);
  } else if (info.kind === 'proposed') {
    const p = info.tx.proposedIn as NonNullable<PipelinePoolTx['proposedIn']>;
    rows.push(['proposal in', `${num(p.block)}${p.uncleIndex !== null ? ' · via uncle' : ''}`]);
    rows.push(['can commit next?', commitNote(p.block, info.tooEarly, s)]);
    foot = `The tx waits in the pool; its proposal sits in block ${num(p.block)}. ${foot}`;
  } else {
    const p = info.tx.proposedIn;
    rows.push(['committed in', num(info.block.number)]);
    rows.push([
      'proposal in',
      `${num(p.block)} (${info.block.number - p.block} blocks earlier)${p.uncleIndex !== null ? ' · via uncle' : ''}`,
    ]);
  }
  const tag = info.kind === 'pending' ? 'pending' : info.kind;
  return {
    key: `tx:${t.hash}`,
    tone: TONE[info.kind],
    title: shortHash(t.hash),
    tag,
    rows,
    foot,
  };
}

export function cellbaseTip(b: PipelineBlock, s: PipelineSnapshot): TipContent {
  const paidBy = s.blocks.find((x) => x.cellbase.pays?.hash === b.hash) ?? null;
  return {
    key: `cellbase:${b.hash}`,
    tone: TONE.committed,
    title: 'Cellbase',
    tag: 'first tx',
    rows: [
      ['block', num(b.number)],
      ['pays reward of', b.cellbase.pays ? num(b.cellbase.pays.number) : 'none yet'],
      ['own reward', paidBy ? `paid by ${num(paidBy.number)}` : 'paid by a later block'],
    ],
    foot: "Every block starts with a cellbase. It mints new CKB and pays the reward of an earlier block — the one whose reward became final here (its finalized_at), once that block's proposal window had closed.",
  };
}

const CHIP_WORDS: Record<PipelineProposal['state'], string> = {
  early: 'waiting · too early',
  waiting: 'waiting',
  committed: 'committed later',
  expired: 'expired',
};

export function chipTip(b: PipelineBlock, p: PipelineProposal, s: PipelineSnapshot): TipContent {
  const far = s.consensus.proposalWindowFarthest;
  const uncle = p.uncleIndex === null ? null : b.uncles[p.uncleIndex];
  const rows: [string, string][] = [
    ['proposal id', `${p.shortId}…`],
    ['in block', num(b.number)],
  ];
  if (uncle) rows.push(['carried by', `uncle ${num(uncle.number)}`]);
  rows.push(['status', CHIP_WORDS[p.state]]);
  if (p.state === 'committed' && p.committedBlock !== null) {
    rows.push([
      'tx committed in',
      `${num(p.committedBlock)} (${p.committedBlock - b.number} blocks later)`,
    ]);
  }
  if (p.state !== 'committed' && p.txHash && s.pool) {
    if (s.pool.pending.some((t) => t.hash === p.txHash)) rows.push(['tx now', 'back in pending']);
    else if (s.pool.proposed.some((t) => t.hash === p.txHash))
      rows.push(['tx now', 'in the pool, proposed']);
  }
  const why: Record<PipelineProposal['state'], string> = {
    early: 'Proposed in this block, so the next block cannot commit it yet.',
    waiting: `Any of the next blocks can commit it, until ${far} blocks after this one.`,
    committed: '',
    expired: `Not committed within ${far} blocks; the pool re-queues or drops the tx.`,
  };
  return {
    key: `chip:${b.hash}:${p.shortId}:${p.uncleIndex ?? ''}`,
    tone: TONE.proposed,
    title: 'Proposal',
    tag: 'part of the block',
    rows,
    foot: `A block carries proposal ids, not the transactions.${uncle ? ' An uncle’s proposals count as if this block had made them.' : ''} ${why[p.state]}`.trim(),
  };
}

export function windowTip(H: number, s: PipelineSnapshot): TipContent {
  const isNext = H === s.tip.number + 1;
  const { from, to } = windowRange(H, s.consensus);
  const rows: [string, string][] = [
    [isNext ? 'next block' : 'block', num(H)],
    [isNext ? 'can pack proposals from' : 'packed proposals from', `${tail4(from)} – ${tail4(to)}`],
  ];
  if (isNext && s.pool) {
    const can = s.pool.proposed.filter(
      (t) => t.proposedIn && inWindow(t.proposedIn.block, H, s.consensus)
    ).length;
    rows.push(['pool txs it can pack', `${can} of ${s.pool.proposed.length}`]);
  }
  const { proposalWindowClosest: c, proposalWindowFarthest: f } = s.consensus;
  return {
    key: `window:${H}`,
    tone: TONE.proposed,
    title: 'Proposal window',
    tag: null,
    rows,
    foot: `A tx can be committed ${c} to ${f} blocks after it is proposed: the latest block is too recent, and proposals older than ${f} blocks have expired.`,
  };
}

export function uncleTip(b: PipelineBlock, index: number): TipContent {
  const u = b.uncles[index];
  const n = b.proposals.filter((p) => p.uncleIndex === index).length;
  return {
    key: `uncle:${b.hash}:${index}`,
    tone: TONE.dim,
    title: `Uncle ${shortHash(u.hash)}`,
    tag: 'not on the chain',
    rows: [
      ['height', num(u.number)],
      ['included by', num(b.number)],
      ['proposals', n ? `${n} · count at ${num(b.number)}` : 'none'],
    ],
    foot: 'An uncle is a valid block that lost the race for its height. A later block carries its header and proposals, not its transactions, and those proposals count as if that block had made them.',
  };
}

export function blockTip(b: PipelineBlock, s: PipelineSnapshot): TipContent {
  const count = { waiting: 0, early: 0, committed: 0, expired: 0 };
  for (const p of b.proposals) count[p.state]++;
  const own = b.proposals.filter((p) => p.uncleIndex === null).length;
  const viaUncle = b.proposals.length - own;
  const { from, to } = windowRange(b.number, s.consensus);
  return {
    key: `block:${b.hash}`,
    tone: TONE.committed,
    title: `Block ${num(b.number)}`,
    tag: 'mined',
    rows: [
      ['transactions', `cellbase + ${fmtInt(b.txCount - 1)}`],
      ['proposals', `${own}${viaUncle ? ` + ${viaUncle} via uncle` : ''}`],
      ['uncles', b.uncles.length ? b.uncles.map((u) => num(u.number)).join(', ') : 'none'],
      ['· waiting', String(count.waiting + count.early)],
      ['· committed later', String(count.committed)],
      ['· expired', String(count.expired)],
      ['bytes', `${fmtBytes(b.bytes)} · ${pct(b.bytes, s.consensus.maxBlockBytes)}`],
      [
        'cycles',
        b.cycles === null
          ? 'still being computed'
          : `${fmtCycles(b.cycles)} · ${pct(b.cycles, s.consensus.maxBlockCycles)}`,
      ],
      ['packed from', `${num(from)} – ${num(to)}`],
      ['parent', shortHash(b.parentHash)],
      ['cellbase pays', b.cellbase.pays ? num(b.cellbase.pays.number) : 'none yet'],
    ],
    foot: null,
  };
}

export function nextTip(s: PipelineSnapshot): TipContent {
  const next = s.next;
  if (!next) throw new Error('pipeline: the NEXT tooltip needs a prediction');
  const H = s.tip.number + 1;
  const { from, to } = windowRange(H, s.consensus);
  return {
    key: 'next',
    tone: TONE.committed,
    title: `Next ${num(H)}`,
    tag: 'not mined',
    rows: [
      ['transactions', `cellbase + ~${fmtInt(next.txs.length + next.omittedTxs)} predicted`],
      ['proposals', `${fmtInt(next.proposals)} pending txs`],
      ['bytes', pct(next.bytes, s.consensus.maxBlockBytes)],
      ['cycles', pct(next.cycles, s.consensus.maxBlockCycles)],
      ['can pack from', `${num(from)} – ${num(to)}`],
      ['parent', num(s.tip.number)],
    ],
    foot: 'Dashed = not yet real: a prediction from the pool and the proposal window.',
  };
}

/** A predicted tx on the NEXT card; `pool` is its pool entry when the listed pool has it. */
export function predictedTip(
  tx: { hash: string; size: number },
  pool: PipelinePoolTx | null,
  s: PipelineSnapshot
): TipContent {
  const rows: [string, string][] = [
    ['size · area', `${fmtBytes(tx.size)} · ${pct(tx.size, s.consensus.maxBlockBytes)}`],
    ['predicted for', num(s.tip.number + 1)],
  ];
  if (pool?.proposedIn) {
    rows.push(['proposal in', num(pool.proposedIn.block)], ['fee rate', feeRate(pool.feeRate)]);
  }
  return {
    key: `ghost:${tx.hash}`,
    tone: TONE.committed,
    title: shortHash(tx.hash),
    tag: 'predicted',
    rows,
    foot: pool
      ? 'Dashed = not yet real: the next block is predicted to commit this pool tx.'
      : 'Dashed = not yet real. Its tx is not in the listed pool txs: the pool list is truncated.',
  };
}

interface PipelineTooltipProps {
  content: TipContent | null;
}

/** The hover explanation, positioned next to the pointer by the canvas (fixed, in a portal). */
export const PipelineTooltip = forwardRef<HTMLDivElement, PipelineTooltipProps>(
  function PipelineTooltip({ content }, ref) {
    if (!content) return null;
    return createPortal(
      <div
        ref={ref}
        data-testid="pipeline-tooltip"
        role="tooltip"
        className="border-base-border bg-base-surface/95 text-text pointer-events-none fixed z-50 w-[268px] rounded-lg border px-3 pb-2.5 pt-2 font-mono text-[11px] leading-[1.55] shadow-[0_12px_32px_rgba(0,0,0,0.5)]"
      >
        <div className="border-base-border text-text-bright mb-1.5 flex items-center justify-between gap-2.5 border-b pb-1.5">
          <span>{content.title}</span>
          {content.tag && (
            <span
              className="whitespace-nowrap rounded border px-1.5 py-[3px] text-[10px] leading-none"
              style={{ color: content.tone, borderColor: content.tone }}
            >
              {content.tag}
            </span>
          )}
        </div>
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-px">
          {content.rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-text-dim">{k}</dt>
              <dd className="text-text-bright m-0 text-right tabular-nums">{v}</dd>
            </div>
          ))}
        </dl>
        {content.foot && (
          <div className="text-text-dim mt-[7px] text-[10px] leading-[1.45]">{content.foot}</div>
        )}
      </div>,
      document.body
    );
  }
);
