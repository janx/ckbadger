'use client';

import { useId, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link, { type LinkLook } from '@/components/ui/link';
import { CkbAmount, TokenAmount } from '@/components/ui/ckb-amount';
import { HeadId } from '@/components/ui/head-id';
import { Prose } from '@/components/ui/prose';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { FiberSplitBar } from '@/components/fiber/fiber-split-bar';
import { FiberChannelTimeline } from '@/components/fiber/fiber-channel-timeline';
import { FiberChannelStep, type FiberStepBox } from '@/components/fiber/fiber-channel-step';
import {
  api,
  type FiberChannel,
  type FiberChannelDetail,
  type FiberChannelEvidence,
  type FiberEpochDelay,
} from '@/lib/api';
import { FIBER_STATE_WORDS, epochDelayWords, fiberDuration, stageEpoch } from '@/lib/fiber-format';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatCount, countNoun } from '@/lib/format-count';
import { u64Hex } from '@/lib/hex-bytes';
import { formatUtc8Date } from '@/lib/utils';
import { BOARD_INKS, inkStyle } from '@/lib/design-tokens';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { ShortId } from '@/components/ui/short-id';
import { unitNode } from '@/components/ui/units';

type Party = FiberChannel['observedParties'][number];

interface Props {
  channel: FiberChannelDetail;
  evidence?: FiberChannelEvidence;
  evidenceError?: string;
  /** The funding token's symbol and decimals, for a token-funded channel (FF-C7). */
  token?: { symbol: string | null; decimals: number | null };
}

/**
 * A decoded `since` epoch delay, e.g. "relative · 1 + 0/1 epochs". The decode comes from our API;
 * a fraction outside its epoch is a broken invariant there, reported with the raw u64.
 */
export function formatEpochDelay(delay: FiberEpochDelay, raw: string): string {
  const { relative, epochs, index, length } = delay;
  if (
    ![epochs, index, length].every((n) => Number.isSafeInteger(n) && n >= 0) ||
    length === 0 ||
    index >= length
  )
    throw new Error(`Fiber delay ${raw}: invalid decoded epoch ${JSON.stringify(delay)}`);
  return `${relative ? 'relative' : 'absolute'} · ${epochs.toLocaleString('en-US')} + ${index}/${length} epoch${epochs === 1 && index === 0 ? '' : 's'}`;
}

/**
 * A party by its address, else its lock hash, at the row size, as the hero's pair and the story
 * name it (R9; U-09: it was the last six characters alone on the split bar and the received
 * lines, then the tight size on the split bar). A split bar's names stand in a line that wraps
 * at the bar's full width, no tile: "ckb1…7sppedrr 1,000.00000569 CKB" is 32 characters, 249.6 px
 * at 13 px.
 */
const partyId = (party: Party) =>
  party.address ? (
    <ShortId value={party.address} kind="address" size="row" />
  ) : (
    <ShortId value={party.lockScriptHash} kind="hash" size="row" />
  );

/**
 * A party in a sentence or a row, at the row size: a link inside the line (`inline`) by default,
 * or the row's key where it heads a row of the party's figures.
 */
function PartyLink({ party, look = 'inline' }: { party: Party; look?: LinkLook }) {
  return party.address ? (
    <Link href={`/address/${party.address}`} look={look}>
      <ShortId value={party.address} kind="address" size="row" />
    </Link>
  ) : (
    <ShortId title={party.lockScriptHash} value={party.lockScriptHash} kind="hash" size="row" />
  );
}

/**
 * The channel's own figures, exact (readability R5: the page of the thing itself), its trailing
 * zeros dropped; a sentence's figure is the summary, its exact value in its title.
 */
const exact = (shannons: string | bigint, signed = false) => (
  <CkbAmount form="exact" shannons={BigInt(shannons)} signed={signed} tone="plain" />
);
const summary = (shannons: string | bigint) => (
  <CkbAmount form="summary" shannons={BigInt(shannons)} signed={false} tone="plain" />
);

/** The opening flows of the two observed parties, in their order, when both put CKB in. */
type Stage = FiberChannelEvidence['stages'][number];
const STAGE_KEYS = {
  open: 'opening',
  close: 'close',
  commitment: 'currentCommitment',
  settle: 'settlement',
} as const;

function stageOf(evidence: FiberChannelEvidence | undefined, key: string): Stage | undefined {
  return evidence?.stages.find((stage) => stage.stage === key);
}

/** The stage that paid the channel out: the settlement, or a cooperative close. */
function finalStage(channel: FiberChannelDetail, evidence: FiberChannelEvidence | undefined) {
  if (channel.state === 'settled') return stageOf(evidence, STAGE_KEYS.settle);
  if (channel.state === 'cooperativelyClosed') return stageOf(evidence, STAGE_KEYS.close);
  return undefined;
}

/**
 * Each observed party's amount in one stage, from the API's exact party flows (R2b-b-16):
 * `sign` −1 reads what it put in (opening), +1 what it received (the close). Only parties with
 * a positive amount are returned, in the channel's party order.
 */
function stageAmounts(channel: FiberChannelDetail, stage: Stage | undefined, sign: 1 | -1) {
  if (!stage) return [];
  return channel.observedParties.flatMap((party) => {
    const flow = stage.partyFlows.find((f) => f.lockScriptHash === party.lockScriptHash);
    const amount = flow ? BigInt(flow.netReceived) * BigInt(sign) : BigInt(0);
    return amount > BigInt(0) ? [{ party, amount }] : [];
  });
}

/** A two-party split, drawable only when both observed parties have a positive amount. */
function split(channel: FiberChannelDetail, amounts: { party: Party; amount: bigint }[]) {
  return channel.observedParties.length === 2 && amounts.length === 2 ? amounts : null;
}

function Summary({
  channel,
  evidence,
}: {
  channel: FiberChannelDetail;
  evidence: FiberChannelEvidence | undefined;
}) {
  const lifetime = channel.observedLifetimeMs
    ? fiberDuration(channel.observedLifetimeMs, `${channel.channelId} lifetime`)
    : null;
  if (lifetime === null) throw new Error(`Fiber channel ${channel.channelId} lacks a lifetime`);
  const putIn = stageAmounts(channel, stageOf(evidence, STAGE_KEYS.open), -1).map(
    ({ party, amount }) => ({ party, net: amount })
  );
  const delay = channel.delayEpochDecoded ? epochDelayWords(channel.delayEpochDecoded) : null;
  const story =
    channel.state === 'open'
      ? `It has been open for ${lifetime}; the payments inside it never touch the chain.`
      : channel.state === 'cooperativelyClosed'
        ? `${lifetime} later the two closed it together in one transaction, which paid out the final split.`
        : channel.state === 'forceClosed'
          ? `${lifetime} later one side closed it without a joint signature: the commitment carries the last agreed split, and it can be paid out after a wait of ${delay}.`
          : `${lifetime} later the channel was closed without a joint signature: the commitment carried the last agreed split, and after a wait of ${delay} the settlement paid it out.`;
  return (
    <p data-testid="channel-summary">
      {/* readability R5: a sentence's figures take the summary form, their exact values in their
          titles and in tier 1's opening flows */}
      {putIn.length > 0 && (
        <>
          <PartyLink party={putIn[0].party} /> put {summary(putIn[0].net)} net into the opening
          transaction
          {putIn.slice(1).map(({ party, net }) => (
            <span key={party.lockScriptHash}>
              {' '}
              and <PartyLink party={party} /> {summary(net)}
            </span>
          ))}
          .{' '}
        </>
      )}
      {story}
    </p>
  );
}

/**
 * The hero's lede (hierarchy v3, R13): the channel's story in one line (≤ 76 characters fill the
 * measure, readability R4); the full story, with what each side put in, is the tier-2 `Summary`.
 */
function ledeSentence(channel: FiberChannelDetail): string {
  const id = channel.channelId;
  const lifetime = fiberDuration(
    required(channel.observedLifetimeMs, 'a lifetime', id),
    `${id} lifetime`
  );
  const delay = () => epochDelayWords(required(channel.delayEpochDecoded, 'a decoded delay', id));
  switch (channel.state) {
    case 'open':
      return `Open for ${lifetime}; its payments never touch the chain.`;
    case 'cooperativelyClosed':
      return `Closed together ${lifetime} after it opened, in one transaction.`;
    case 'forceClosed':
      return `Force-closed ${lifetime} after it opened; the payout waits ${delay()}.`;
    case 'settled':
      return `Force-closed ${lifetime} after it opened; settled after ${delay()}.`;
  }
}

/** The channel's capacity as the page's display figure (R4), in its summary form (R5). */
function CapacityFigure({ channel }: { channel: FiberChannelDetail }) {
  return (
    <div data-role="capacity" className="text-text-bright text-index sm:text-display">
      <CkbAmount
        form="summary"
        shannons={parseShannons(channel.capacity, `${channel.channelId} capacity`)}
        signed={false}
        tone="inherit"
        unit={false}
      />
      {/* The space takes the unit's size, so " CKB" ends inside a 358 px column at 390. */}
      <span className="text-body text-text-dim font-normal"> CKB</span>
    </div>
  );
}

const STEP_WORDS = ['no', 'one', 'two', 'three', 'four'];

/**
 * "ckb1…7sppedrr received 899.99 CKB net" for each party paid in the final stage: a line of a
 * step's box, a sentence, so the summary form (R5), its exact value in its title.
 */
function receivedLines(channel: FiberChannelDetail, stage: Stage | undefined): ReactNode[] {
  return stageAmounts(channel, stage, 1).map(({ party, amount }) => (
    <span key={party.lockScriptHash}>
      {partyId(party)} received {summary(amount)} net
    </span>
  ));
}

type Split = { party: Party; amount: bigint }[];

/**
 * What moved inside the channel, in one sentence (the readability plan's P4: "the Fiber channel
 * page states what moved in a sentence"), from the stages' own party flows (the API's figures,
 * the ones the two split bars draw): what each party put in at the open and, once the channel
 * paid out to both, what each got back at the close or the settlement. Nothing is added or
 * subtracted in the page: the chain does not say which side paid the fees inside both figures, so
 * the amount that changed sides is not a figure the page can state; the two figures stand side by
 * side for each party instead. Summary form (R5: a sentence), each cut figure titled exact. Under
 * 140 characters for two parties with seven-digit amounts (not a tier-1 prose block).
 */
function WhatMoved({
  channel,
  atOpen,
  atClose,
}: {
  channel: FiberChannelDetail;
  atOpen: Split;
  atClose: Split | null;
}) {
  if (atClose)
    atClose.forEach(({ party }, i) => {
      if (party.lockScriptHash !== atOpen[i].party.lockScriptHash)
        throw new Error(
          `Fiber channel ${channel.channelId}: the close's party ${party.lockScriptHash} is not the open's ${atOpen[i].party.lockScriptHash}`
        );
    });
  const unpaid = channel.state === 'open' || channel.state === 'forceClosed';
  return (
    <p data-testid="what-moved" className="text-text text-body max-w-measure">
      {atClose ? (
        atOpen.map(({ party, amount }, i) => (
          <span key={party.lockScriptHash}>
            {i > 0 && '; '}
            <PartyLink party={party} /> put in {summary(amount)} and got back{' '}
            {summary(atClose[i].amount)}
          </span>
        ))
      ) : (
        <>
          <PartyLink party={atOpen[0].party} /> put in {summary(atOpen[0].amount)} and{' '}
          <PartyLink party={atOpen[1].party} /> {summary(atOpen[1].amount)}
          {unpaid && '; nothing has been paid out yet'}
        </>
      )}
      .
    </p>
  );
}

/**
 * A force-closed channel that waits (review fiber-channel-open: "the live commitment is the key
 * fact, set out like every other card"): the commitment cell that holds the channel now, by its
 * out point, what it holds and when it can pay out, first in tier 1. How the CKB inside it splits
 * between the two parties is not on chain while the cell is live (lane exactness, part A, A5 iv):
 * a commitment lock of the 57- or 58-byte layouts holds a 20-byte hash of the settlement, the split
 * both last signed, and the settlement transaction that spends the cell reveals it and pays it
 * out; the older layouts hold not even that. So no field can state it, and the page says what the
 * chain holds instead.
 */
function LiveCommitment({
  channel,
  commitment,
}: {
  channel: FiberChannelDetail;
  commitment: NonNullable<FiberChannelEvidence['currentCommitment']>;
}) {
  const delay = required(channel.delayEpochDecoded, 'a decoded delay', channel.channelId);
  return (
    <p data-testid="live-commitment" className="text-text text-body max-w-measure">
      The live commitment{' '}
      <Link
        look="inline"
        href={`/cell/${commitment.txHash}-${commitment.outputIndex}`}
        title={`${commitment.txHash}:${commitment.outputIndex}`}
      >
        <ShortId value={commitment.txHash} kind="hash" size="row" />
        <OutpointIndex index={commitment.outputIndex} />
      </Link>{' '}
      holds {summary(commitment.capacity)}. How it divides between the two is not on chain:{' '}
      {commitment.settlementHash
        ? 'the lock holds only a 20-byte hash of the split both last signed, and the settlement that spends this cell shows the amounts.'
        : 'the settlement that spends this cell shows the amounts.'}{' '}
      {commitment.readyEpoch
        ? `The payout can be spent from epoch ${stageEpoch(commitment.readyEpoch, commitment.blockNumber)}.`
        : `The payout waits ${epochDelayWords(delay)}${delay.relative ? ' after the commitment' : ''}.`}
    </p>
  );
}

interface StepSpec {
  key: string;
  title: string;
  txHash: string;
  blockNumber: number;
  timestamp: string;
  description: string;
  /** The evidence stage key, for the step's epoch. */
  stage: (typeof STAGE_KEYS)[keyof typeof STAGE_KEYS];
  consumes?: { txHash: string; index: number; role: string };
  carries?: { index: number; role: string };
  box: FiberStepBox;
}

function required<T>(value: T | null, what: string, channelId: string): T {
  if (value === null) throw new Error(`Fiber channel ${channelId} lacks ${what}`);
  return value;
}

/** The channel's on-chain steps, each tied to the next by the cell that carries the channel. */
function channelSteps(
  channel: FiberChannelDetail,
  evidence: FiberChannelEvidence | undefined
): StepSpec[] {
  const id = channel.channelId;
  const lifetime = fiberDuration(
    required(channel.observedLifetimeMs, 'a lifetime', id),
    `${id} lifetime`
  );
  const forced = channel.state === 'forceClosed' || channel.state === 'settled';
  const delay = forced
    ? epochDelayWords(required(channel.delayEpochDecoded, 'a decoded delay', id))
    : null;
  const tracked = evidence?.currentCommitment ?? null;
  const updated = evidence?.stages.find((stage) => stage.stage === 'currentCommitment');
  if (updated && (!tracked || updated.txHash !== tracked.txHash))
    throw new Error(
      `Fiber channel ${id}: current commitment stage differs from tracked commitment`
    );
  const funding = { txHash: channel.fundingTxHash, index: channel.fundingOutputIndex };
  const steps: StepSpec[] = [
    {
      key: 'open',
      stage: STAGE_KEYS.open,
      title: 'Open',
      txHash: channel.fundingTxHash,
      blockNumber: channel.openBlock,
      timestamp: channel.openTimestamp,
      description: `The parties put CKB${channel.udtTypeHash ? ' and the token' : ''} into one cell under Fiber Funding Lock, which only the two of them together can unlock.`,
      carries: { index: channel.fundingOutputIndex, role: 'Fiber funding' },
      box:
        channel.state === 'open'
          ? {
              head: `Still open after ${lifetime}`,
              lines: [
                'the funding cell holds the channel',
                'while the two pay each other off chain',
              ],
            }
          : {
              head: `→ step 2, ${lifetime} later`,
              lines: [
                'the funding cell held the channel',
                'while the two paid each other off chain',
              ],
            },
    },
  ];
  if (channel.closeTxHash) {
    const carriesCommitment =
      forced && tracked && tracked.txHash === channel.closeTxHash
        ? { index: tracked.outputIndex, role: 'Fiber commitment' }
        : undefined;
    steps.push({
      key: 'close',
      stage: STAGE_KEYS.close,
      title: forced ? 'Force close' : 'Close',
      txHash: channel.closeTxHash,
      blockNumber: required(channel.closeBlock, 'a close block', id),
      timestamp: required(channel.closeTimestamp, 'a close time', id),
      // R13 / R14: one caption per step; what the wait is for is said in tier 2.
      description: forced
        ? 'One side posted the commitment: the last state both had signed.'
        : 'Both signed one transaction that spent the funding cell and paid out the final split.',
      consumes: { ...funding, role: 'Fiber funding' },
      carries: carriesCommitment,
      box: !forced
        ? {
            head: 'Closed',
            lines: [
              ...receivedLines(channel, finalStage(channel, evidence)),
              'the channel no longer exists on chain',
            ],
          }
        : channel.state === 'settled'
          ? {
              head: `→ step ${updated ? 4 : 3}, after ${delay}`,
              lines: [
                'the commitment lock lets the latest state',
                `be challenged for ${delay} before payout`,
              ],
            }
          : {
              head: '→ settlement, not yet',
              lines: [
                tracked?.readyEpoch
                  ? `the payout can be spent from epoch ${tracked.readyEpoch}`
                  : `the payout waits ${delay} after the commitment`,
              ],
            },
    });
  }
  if (updated && tracked)
    steps.push({
      key: 'commitment',
      stage: STAGE_KEYS.commitment,
      title: 'Updated commitment',
      txHash: updated.txHash,
      blockNumber: updated.blockNumber,
      timestamp: updated.timestamp,
      description:
        'A partial settlement replaced the commitment cell; earlier replacements are not indexed as channel stages.',
      carries: { index: tracked.outputIndex, role: 'Fiber commitment' },
      box:
        channel.state === 'settled'
          ? { head: '→ the settlement', lines: [] }
          : { head: '→ settlement, not yet', lines: ['the latest commitment is still live'] },
    });
  if (channel.settlementTxHash)
    steps.push({
      key: 'settle',
      stage: STAGE_KEYS.settle,
      title: 'Settle',
      txHash: channel.settlementTxHash,
      blockNumber: required(channel.settlementBlock, 'a settlement block', id),
      timestamp: required(channel.settlementTimestamp, 'a settlement time', id),
      description:
        'After the wait, the commitment cell was spent and the last agreed split paid out.',
      consumes: tracked
        ? { txHash: tracked.txHash, index: tracked.outputIndex, role: 'Fiber commitment' }
        : undefined,
      box: {
        head: 'Closed',
        lines: [
          ...receivedLines(channel, finalStage(channel, evidence)),
          'the channel no longer exists on chain',
        ],
      },
    });
  return steps;
}

const STAGE_WORDS: Record<string, string> = {
  opening: 'open',
  close: 'close',
  currentCommitment: 'updated commitment',
  settlement: 'settle',
};

function StageWitnesses({ stage }: { stage: FiberChannelEvidence['stages'][number] }) {
  const tx = useQuery({
    queryKey: ['cell-transaction-detail', stage.txHash],
    queryFn: () => api.getTransactionDetail(stage.txHash),
  });
  const sizes = tx.data?.witnessDetails?.map((witness) => witness.size);
  return (
    <span>
      {unitNode(formatCount(stage.witnessCount, 'witness', 'witnesses'))}
      {sizes && sizes.length > 0 && `, ${sizes.join(', ')} B`}
      {` · fee ${BigInt(stage.fee).toLocaleString('en-US')} shannons, ${
        stage.feeSource === 'channel_only' ? 'out of the channel' : 'from outside inputs'
      }`}
    </span>
  );
}

function Grid({ children }: { children: ReactNode }) {
  return (
    <div className="text-small grid min-w-0 grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-[150px_minmax(0,1fr)]">
      {children}
    </div>
  );
}

export function FiberChannelBoard({ channel, evidence, evidenceError, token }: Props) {
  const tracked = evidence?.currentCommitment ?? null;
  const delayBytes = tracked
    ? u64Hex(tracked.delayEpoch, `Fiber commitment ${tracked.txHash} delay`)
    : null;
  const atOpen = split(channel, stageAmounts(channel, stageOf(evidence, STAGE_KEYS.open), -1));
  const closing = finalStage(channel, evidence);
  const atClose = split(channel, stageAmounts(channel, closing, 1));
  // a force-closed channel that waits: its commitment cell holds the channel now
  const live = channel.state === 'forceClosed' && tracked !== null && tracked.isLive;
  const commitmentStage =
    stageOf(evidence, STAGE_KEYS.commitment) ??
    (channel.state === 'forceClosed' || channel.state === 'settled'
      ? stageOf(evidence, STAGE_KEYS.close)
      : undefined);
  const settlement = stageOf(evidence, STAGE_KEYS.settle);
  const waitParts = [
    commitmentStage &&
      `commitment at epoch ${stageEpoch(commitmentStage.epoch, commitmentStage.blockNumber)}`,
    tracked?.readyEpoch && `earliest payout at epoch ${tracked.readyEpoch}`,
    settlement && `settled at epoch ${stageEpoch(settlement.epoch, settlement.blockNumber)}`,
  ].filter((part): part is string => Boolean(part));
  const steps = channelSteps(channel, evidence);
  const [first, second] = channel.observedParties;
  const argsBytes = (hex: string) => (hex.length - 2) / 2;
  const partyOf = (lockHash: string) =>
    channel.observedParties.find((party) => party.lockScriptHash === lockHash);
  const titleId = useId();
  const flows = evidence?.openingPartyFlows ?? [];
  // Tier 2 (F2): every sentence that left tier 1, beside the lock evidence when it has loaded.
  const words = (
    <DetailSection title="The channel, in words">
      <Prose>
        <Summary channel={channel} evidence={evidence} />
        {(channel.state === 'forceClosed' || channel.state === 'settled') && (
          <p>
            A force close moves the funding cell to a commitment lock with a wait of{' '}
            {epochDelayWords(
              required(channel.delayEpochDecoded, 'a decoded delay', channel.channelId)
            )}
            , so the other side can answer with a newer state.
          </p>
        )}
        {atOpen && (
          <p>
            The chain never sees a Fiber payment, only the split when the channel opens and the
            split when it closes.
            {atClose &&
              ' Both splits include fees, and the chain does not say which side paid them, so the amount that changed sides is not shown.'}
          </p>
        )}
        {/* The caveat of tier 1's "Opening transaction, per party" whole, on the measure (it was
            one tier-2 line of 187 characters over the grid; review fiber-channel). */}
        {flows.length > 0 && (
          <p>
            The opening transaction&apos;s parties are its non-Fiber locks, on its inputs or
            outputs. An output-only lock did not necessarily sign it. Net input includes fees and
            any unrelated outputs in the same transaction; it is not a proven channel balance or
            payment.
          </p>
        )}
      </Prose>
    </DetailSection>
  );
  return (
    <div>
      {/* Hierarchy v3 (R13): the hero — title, id, one lede, the capacity and the pair — then
          each section's evidence first with one caption; the prose is tier 2. */}
      <div data-tier="1" className="min-w-0">
        <section aria-labelledby={titleId} className="min-w-0">
          <div className="flex flex-wrap items-baseline gap-3">
            {/* PB-06 / PA-32: an entity h1, one type step smaller below md (30 → 26 px). */}
            <h1 id={titleId} className="text-text-bright text-entity max-md:text-index font-medium">
              Fiber channel
            </h1>
            <span className="text-text-dim text-body">{FIBER_STATE_WORDS[channel.state]}</span>
          </div>
          {/* Lane P4 (review fiber-channel: "the full channel id at 11 px dim under the title"):
              an id is a value, in the reading ink at the small step, in the head's designed lines
              (one line where its 514.8 px fit, else the two halves, 0x and 32 digits over 32:
              265.2 px at 13 px, inside a 358 px column with the copy's 32 px box and 8 px gap),
              with the head's one copy. It was a quiet paragraph broken anywhere. */}
          <div data-testid="channel-id" className="text-text text-small mt-1.5 min-w-0">
            <HeadId value={channel.channelId} kind="hash" copy="copy channel id" />
          </div>
          <p className="text-text text-body max-w-measure mt-1.5">{ledeSentence(channel)}</p>
          <div data-role="figure" className="mt-9 min-w-0">
            <div className="eyebrow text-text-dim text-meta mb-1.5 w-fit">capacity</div>
            <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
              <CapacityFigure channel={channel} />
              <p data-testid="channel-pair" className="text-body max-w-measure min-w-0">
                {first && <PartyLink party={first} />}
                {second && (
                  <>
                    {' '}
                    <span className="text-[color:var(--ink)]" style={inkStyle(BOARD_INKS.quiet)}>
                      ↔
                    </span>{' '}
                    <PartyLink party={second} />
                  </>
                )}
                {channel.udtTypeHash && channel.udtAmount !== null && token && (
                  <>
                    {' '}
                    <span className="text-text-dim">· holds</span>{' '}
                    <TokenAmount
                      form="summary"
                      amount={channel.udtAmount}
                      decimals={token.decimals}
                    />{' '}
                    {token.symbol ?? 'tokens'}
                  </>
                )}
              </p>
            </div>
          </div>
          {evidenceError && (
            <p
              role="alert"
              className="text-rouge text-small max-w-measure mt-3 [overflow-wrap:anywhere]"
            >
              Local node evidence unavailable: {evidenceError}
            </p>
          )}
        </section>

        <div className="mt-16 flex flex-col gap-[72px]">
          {(atOpen || live) && (
            <DetailSection title="What moved inside the channel">
              {live && tracked && <LiveCommitment channel={channel} commitment={tracked} />}
              {atOpen && (
                <>
                  <div className={live ? 'mt-3.5' : undefined}>
                    <WhatMoved channel={channel} atOpen={atOpen} atClose={atClose} />
                  </div>
                  <div className="mt-[18px] flex flex-col gap-4">
                    <FiberSplitBar
                      left={{ name: partyId(atOpen[0].party), shannons: atOpen[0].amount }}
                      right={{ name: partyId(atOpen[1].party), shannons: atOpen[1].amount }}
                      note={`at open, ${formatUtc8Date(channel.openTimestamp)} · net input to the opening transaction, its fee included`}
                    />
                    {atClose && closing && (
                      <FiberSplitBar
                        left={{ name: partyId(atClose[0].party), shannons: atClose[0].amount }}
                        right={{
                          name: partyId(atClose[1].party),
                          shannons: atClose[1].amount,
                        }}
                        note={`at the close, ${formatUtc8Date(closing.timestamp)} · net received in the ${
                          closing.stage === STAGE_KEYS.settle ? 'settlement' : 'closing'
                        } transaction, payout and change alike`}
                      />
                    )}
                  </div>
                  <p data-role="caption" className="text-text text-body max-w-measure mt-3.5">
                    The chain sees no Fiber payment, only the split at the open and the close.
                  </p>
                </>
              )}
            </DetailSection>
          )}

          {/* The plan's P4 sentence: per-party in, back and net in tier 1, the API's opening flows
              (they were tier 2's, under a 187-character line of caveat); the caveat whole is tier
              2's, one line of it here. The party is the row's key; the three figures are the
              channel's own, exact, each part held whole on its line. */}
          {flows.length > 0 && (
            <DetailSection title="Opening transaction, per party">
              <Grid>
                {flows.map((flow) => {
                  const party = partyOf(flow.lockScriptHash);
                  return (
                    <span key={flow.lockScriptHash} className="contents">
                      <span>
                        {party ? (
                          <PartyLink party={party} look="key" />
                        ) : (
                          <ShortId value={flow.lockScriptHash} kind="hash" size="row" />
                        )}
                      </span>
                      <span className="text-text">
                        <span data-flow="in" className="whitespace-nowrap">
                          in {exact(flow.openingInputCapacity)}
                        </span>{' '}
                        ·{' '}
                        <span data-flow="back" className="whitespace-nowrap">
                          back {exact(flow.openingOutputCapacity)}
                        </span>{' '}
                        ·{' '}
                        <span data-flow="net" className="whitespace-nowrap">
                          net {exact(flow.observedNetInput, true)}
                        </span>
                      </span>
                    </span>
                  );
                })}
              </Grid>
              <p data-role="caption" className="text-text text-body max-w-measure mt-3.5">
                Net input includes the fee and any unrelated outputs: not a proven balance.
              </p>
            </DetailSection>
          )}

          {/* Rubbing × Song print, phase 5 (D14): on chain, run on its steps - so far, while the
              channel can still take another. The plan's P4: the timeline stays in tier 1; each
              step's transaction, drawn whole, is tier 2's ("Each step, as a transaction"). */}
          <DetailSection
            title="On chain"
            args={
              channel.state === 'open' || channel.state === 'forceClosed'
                ? `${STEP_WORDS[steps.length]} ${countNoun(steps.length, 'step')} so far`
                : `${STEP_WORDS[steps.length]} ${countNoun(steps.length, 'step')}`
            }
          >
            <FiberChannelTimeline channel={channel} />
          </DetailSection>
        </div>
      </div>

      <div id="evidence" className="scroll-mt-4">
        <DetailTier2>
          {/* The plan's P4: each step's transaction, drawn whole with its cells, steps back from
              tier 1 (review fiber-channel: every cell's lock, size and state, and the owner's
              stone, the most saturated mark of the first screen); tier 1 keeps the timeline. */}
          <DetailSection title="Each step, as a transaction">
            <div className="flex flex-col gap-[22px]">
              {steps.map((step, index) => (
                <FiberChannelStep
                  key={step.key}
                  n={index + 1}
                  title={step.title}
                  txHash={step.txHash}
                  blockNumber={step.blockNumber}
                  timestamp={step.timestamp}
                  description={step.description}
                  epoch={stageOf(evidence, step.stage)?.epoch}
                  consumes={step.consumes}
                  carries={step.carries}
                  box={step.box}
                />
              ))}
            </div>
          </DetailSection>
          {!evidence && words}
          {evidence && (
            <>
              <div className="grid min-w-0 items-start gap-x-14 gap-y-10 lg:grid-cols-2">
                <div data-column="left" className="min-w-0">
                  <DetailSection title="The two locks">
                    <div className="text-small flex flex-col gap-3">
                      <div>
                        <div className="text-text-dim">
                          Fiber Funding Lock · args,{' '}
                          {unitNode(formatCount(argsBytes(evidence.funding.lockArgs), 'byte'))}
                        </div>
                        <div className="text-text-bright break-all">
                          {evidence.funding.lockArgs}
                        </div>
                        <div className="text-text-dim">
                          blake160 of the two parties’ aggregated public key
                        </div>
                        <Link
                          href={`/script/${evidence.funding.lockCodeHash}?hashType=${evidence.funding.lockHashType}`}
                          look="inline"
                          className="break-all"
                        >
                          {evidence.funding.lockCodeHash}
                        </Link>
                      </div>
                      {tracked ? (
                        <div>
                          <div className="text-text-dim mb-1">
                            Fiber Commitment Lock · args,{' '}
                            {unitNode(formatCount(argsBytes(tracked.lockArgs), 'byte'))} ·{' '}
                            {tracked.lockFormat}
                          </div>
                          <Grid>
                            <span className="text-text-dim">Current commitment</span>
                            <Link
                              href={`/cell/${tracked.txHash}-${tracked.outputIndex}`}
                              look="inline"
                              className="min-w-0 break-all"
                            >
                              {tracked.txHash}
                              <OutpointIndex index={tracked.outputIndex} />
                            </Link>
                            <span className="text-text-dim">Pubkey hash · 20</span>
                            <span className="text-text-bright break-all">{tracked.pubkeyHash}</span>
                            <span className="text-text-dim">Delay · 8 bytes, LE</span>
                            <span className="text-text-bright min-w-0">
                              <span className="break-all">
                                {delayBytes!.littleEndian} = {delayBytes!.value}
                              </span>{' '}
                              · {formatEpochDelay(tracked.delayEpochDecoded, tracked.delayEpoch)}
                            </span>
                            <span className="text-text-dim">Version · exact u64</span>
                            <span className="text-text-bright break-all">{tracked.version}</span>
                            {tracked.settlementHash !== null && (
                              <>
                                <span className="text-text-dim">Settlement hash / flag</span>
                                <span className="text-text-bright break-all">
                                  {tracked.settlementHash ?? 'none'} ·{' '}
                                  {tracked.settlementFlag ?? 'none'}
                                </span>
                              </>
                            )}
                            <span className="text-text-dim">Commitment script</span>
                            <Link
                              href={`/script/${tracked.lockCodeHash}?hashType=${tracked.lockHashType}`}
                              look="inline"
                              className="min-w-0 break-all"
                            >
                              {tracked.lockCodeHash}
                            </Link>
                          </Grid>
                        </div>
                      ) : (
                        <p className="text-text">No commitment cell has appeared on chain.</p>
                      )}
                    </div>
                  </DetailSection>
                </div>
                <div data-column="right" className="min-w-0 space-y-8">
                  {words}
                  <DetailSection title="Witnesses and timing">
                    <Grid>
                      {evidence.stages.map((stage) => (
                        <span key={stage.txHash} className="contents">
                          <span className="text-text-dim">
                            {stage.stage === STAGE_KEYS.close &&
                            (channel.state === 'forceClosed' || channel.state === 'settled')
                              ? 'force close'
                              : (STAGE_WORDS[stage.stage] ?? stage.stage)}
                          </span>
                          <StageWitnesses stage={stage} />
                        </span>
                      ))}
                      {waitParts.length > 0 && (
                        <>
                          <span className="text-text-dim">the wait</span>
                          <span className="text-text">{waitParts.join(' · ')}</span>
                        </>
                      )}
                      {tracked && (
                        <>
                          <span className="text-text-dim">delay rule</span>
                          <span className="text-text">
                            {tracked.epochLock === 'firstSettlementDelay'
                              ? `first settlement delay ${tracked.epochLockMetAtTip ? 'met' : 'waiting'}: ready at epoch ${tracked.readyEpoch}; indexed tip at ${tracked.tipEpoch}`
                              : tracked.epochLock === 'subsequentSettlementNoReappliedDelay'
                                ? 'Subsequent settlement cell: original delay is not reapplied; other transaction conditions remain.'
                                : `Legacy ${argsBytes(tracked.lockArgs)}-byte form: the deployed delay rule is not asserted here.`}
                          </span>
                        </>
                      )}
                    </Grid>
                  </DetailSection>
                </div>
              </div>
              {evidence.settlementOutputs.length > 0 && (
                <DetailSection title="Final transaction outputs">
                  <p className="text-text text-body max-w-measure mb-2">
                    All final transaction outputs are shown, the payouts and any change alike.
                  </p>
                  <Grid>
                    {evidence.settlementOutputs.map((output) => {
                      const party = output.observedParty
                        ? partyOf(output.ownerLockHash)
                        : undefined;
                      if (output.observedParty && !party)
                        throw new Error(
                          `Fiber settlement output ${output.txHash}:${output.outputIndex} names an unlisted party ${output.ownerLockHash}`
                        );
                      return (
                        <span key={output.outputIndex} className="contents">
                          <Link href={`/cell/${output.txHash}-${output.outputIndex}`} look="key">
                            out #{output.outputIndex}
                          </Link>
                          <span className="text-text">
                            {party ? (
                              <PartyLink party={party} />
                            ) : (
                              <span title={output.ownerLockHash}>
                                Other lock ·{' '}
                                <ShortId value={output.ownerLockHash} kind="hash" size="row" />
                              </span>
                            )}{' '}
                            · {exact(output.capacity)}
                          </span>
                        </span>
                      );
                    })}
                  </Grid>
                </DetailSection>
              )}
            </>
          )}
        </DetailTier2>
      </div>
    </div>
  );
}
