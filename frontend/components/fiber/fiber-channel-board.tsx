'use client';

import { useId, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { ExactCkb } from '@/components/activity-event-row';
import { AssetAmount } from '@/components/asset/inventory-table';
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
import { parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatCount } from '@/lib/format-count';
import { u64Hex } from '@/lib/hex-bytes';
import { formatUtc8Date } from '@/lib/utils';
import { BOARD_INKS, inkStyle } from '@/lib/design-tokens';
import { OutpointIndex } from '@/components/ui/outpoint-index';

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

/** A hash or output link in the boards' link cyan; the global `a:hover` jade still wins. */
const LINK = inkStyle(BOARD_INKS.link);

const shortAddress = (party: Party) =>
  party.address
    ? `${party.address.slice(0, 10)}…${party.address.slice(-6)}`
    : `${party.lockScriptHash.slice(0, 10)}…${party.lockScriptHash.slice(-6)}`;
const tail = (party: Party) => (party.address ?? party.lockScriptHash).slice(-6);

function PartyLink({ party, className }: { party: Party; className?: string }) {
  return party.address ? (
    <Link href={`/address/${party.address}`} className={className ?? 'hover:text-jade'}>
      {shortAddress(party)}
    </Link>
  ) : (
    <span title={party.lockScriptHash}>{shortAddress(party)}</span>
  );
}

const ckb = (shannons: string | bigint, signed = false) => (
  <ExactCkb shannons={BigInt(shannons)} signed={signed} tone="plain" />
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
      {putIn.length > 0 && (
        <>
          <PartyLink party={putIn[0].party} /> put {ckb(putIn[0].net)} net into the opening
          transaction
          {putIn.slice(1).map(({ party, net }) => (
            <span key={party.lockScriptHash}>
              {' '}
              and <PartyLink party={party} /> {ckb(net)}
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
 * The hero's lede (hierarchy v3, R13): the channel's story in one line (≤ 67 characters fill the
 * 560 px lede at 13 px); the
 * full story, with what each side put in, is the tier-2 `Summary`.
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

/** The channel's capacity as the page's display figure (R4): exact, its decimals a step quieter. */
function CapacityFigure({ channel }: { channel: FiberChannelDetail }) {
  const parts = splitCkbAmount(parseShannons(channel.capacity, `${channel.channelId} capacity`));
  return (
    <div data-role="capacity" className="text-text-bright text-index sm:text-display">
      <span className="whitespace-nowrap tabular-nums">
        {parts.integer}
        <span className="text-hero text-text-dim font-normal">
          {parts.fraction}
          <span className="opacity-50">{parts.ghost}</span>
        </span>
      </span>
      {/* The space takes the unit's size, so " CKB" ends inside a 358 px column at 390. */}
      <span className="text-body text-text-dim font-normal"> CKB</span>
    </div>
  );
}

const STEP_WORDS = ['no', 'one', 'two', 'three', 'four'];

/** "ppedrr received 899.99998733 CKB net" for each party paid in the final stage. */
function receivedLines(channel: FiberChannelDetail, stage: Stage | undefined): ReactNode[] {
  return stageAmounts(channel, stage, 1).map(({ party, amount }) => (
    <span key={party.lockScriptHash}>
      {tail(party)} received {ckb(amount)} net
    </span>
  ));
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
      {formatCount(stage.witnessCount, 'witness', 'witnesses')}
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
            <h1 id={titleId} className="text-text-bright text-entity font-medium">
              Fiber channel
            </h1>
            <span className="text-text-dim text-body">{FIBER_STATE_WORDS[channel.state]}</span>
          </div>
          <p className="text-text-dim text-small mt-1.5 w-fit max-w-full break-all">
            {channel.channelId}
          </p>
          <p className="text-text-dim text-body mt-1.5 max-w-[560px]">{ledeSentence(channel)}</p>
          <div data-role="figure" className="mt-9 min-w-0">
            <div className="eyebrow text-text-dim text-meta mb-1.5 w-fit">capacity</div>
            <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
              <CapacityFigure channel={channel} />
              <p data-testid="channel-pair" className="text-body min-w-0">
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
                    <AssetAmount amount={channel.udtAmount} decimals={token.decimals} />{' '}
                    {token.symbol ?? 'tokens'}
                  </>
                )}
              </p>
            </div>
          </div>
          {evidenceError && (
            <p role="alert" className="text-rouge text-small mt-3 break-all">
              Local node evidence unavailable: {evidenceError}
            </p>
          )}
        </section>

        <div className="mt-16 flex flex-col gap-[72px]">
          {atOpen && (
            <DetailSection title="What moved inside the channel">
              <div className="flex flex-col gap-4">
                <FiberSplitBar
                  left={{ name: tail(atOpen[0].party), shannons: atOpen[0].amount }}
                  right={{ name: tail(atOpen[1].party), shannons: atOpen[1].amount }}
                  note={`at open, ${formatUtc8Date(channel.openTimestamp)} · net input to the opening transaction, its fee included`}
                />
                {atClose && closing && (
                  <FiberSplitBar
                    left={{ name: tail(atClose[0].party), shannons: atClose[0].amount }}
                    right={{ name: tail(atClose[1].party), shannons: atClose[1].amount }}
                    note={`at the close, ${formatUtc8Date(closing.timestamp)} · net received in the ${
                      closing.stage === STAGE_KEYS.settle ? 'settlement' : 'closing'
                    } transaction, payout and change alike`}
                  />
                )}
              </div>
              <p data-role="caption" className="text-text text-body mt-3.5 max-w-[640px]">
                The chain sees no Fiber payment, only the split at the open and at the close.
              </p>
            </DetailSection>
          )}

          {/* Rubbing × Song print, phase 5 (D14): on chain, run on its steps - so far, while the
              channel can still take another. */}
          <DetailSection
            title="On chain"
            args={
              channel.state === 'open' || channel.state === 'forceClosed'
                ? `${STEP_WORDS[steps.length]} step${steps.length === 1 ? '' : 's'} so far`
                : `${STEP_WORDS[steps.length]} steps`
            }
          >
            <div className="flex flex-col gap-[22px]">
              <FiberChannelTimeline channel={channel} />
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
        </div>
      </div>

      <div id="evidence" className="scroll-mt-4">
        <DetailTier2>
          {!evidence && words}
          {evidence && (
            <>
              <div className="grid min-w-0 items-start gap-x-14 gap-y-10 lg:grid-cols-2">
                <div data-column="left" className="min-w-0">
                  <DetailSection title="The two locks">
                    <div className="text-small flex flex-col gap-3">
                      <div>
                        <div className="text-text-dim">
                          Fiber Funding Lock · args, {argsBytes(evidence.funding.lockArgs)} bytes
                        </div>
                        <div className="text-text-bright break-all">
                          {evidence.funding.lockArgs}
                        </div>
                        <div className="text-text-dim">
                          blake160 of the two parties’ aggregated public key
                        </div>
                        <Link
                          href={`/script/${evidence.funding.lockCodeHash}?hashType=${evidence.funding.lockHashType}`}
                          className="break-all text-[color:var(--ink)]"
                          style={LINK}
                        >
                          {evidence.funding.lockCodeHash}
                        </Link>
                      </div>
                      {tracked ? (
                        <div>
                          <div className="text-text-dim mb-1">
                            Fiber Commitment Lock · args, {argsBytes(tracked.lockArgs)} bytes ·{' '}
                            {tracked.lockFormat}
                          </div>
                          <Grid>
                            <span className="text-text-dim">Current commitment</span>
                            <Link
                              href={`/cell/${tracked.txHash}-${tracked.outputIndex}`}
                              className="min-w-0 break-all text-[color:var(--ink)]"
                              style={LINK}
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
                              className="min-w-0 break-all text-[color:var(--ink)]"
                              style={LINK}
                            >
                              {tracked.lockCodeHash}
                            </Link>
                          </Grid>
                        </div>
                      ) : (
                        <p className="text-text-dim">No commitment cell has appeared on chain.</p>
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
              {evidence.openingPartyFlows.length > 0 && (
                <DetailSection title="Opening transaction, per party">
                  <p className="text-text-dim text-meta mb-2">
                    Non-Fiber locks on the opening transaction’s inputs or outputs. An output-only
                    lock did not necessarily sign it. Net input includes fees and any unrelated
                    outputs in the same transaction; it is not a proven channel balance or payment.
                  </p>
                  <Grid>
                    {evidence.openingPartyFlows.map((flow) => {
                      const party = partyOf(flow.lockScriptHash);
                      return (
                        <span key={flow.lockScriptHash} className="contents">
                          <span className="text-text-dim">
                            {party ? (
                              <PartyLink party={party} />
                            ) : (
                              `${flow.lockScriptHash.slice(0, 10)}…`
                            )}
                          </span>
                          <span className="text-text">
                            <span data-flow="in" className="whitespace-nowrap">
                              in {ckb(flow.openingInputCapacity)}
                            </span>{' '}
                            ·{' '}
                            <span data-flow="back" className="whitespace-nowrap">
                              back {ckb(flow.openingOutputCapacity)}
                            </span>{' '}
                            ·{' '}
                            <span data-flow="net" className="whitespace-nowrap">
                              net {ckb(flow.observedNetInput, true)}
                            </span>
                          </span>
                        </span>
                      );
                    })}
                  </Grid>
                </DetailSection>
              )}
              {evidence.settlementOutputs.length > 0 && (
                <DetailSection title="Final transaction outputs">
                  <p className="text-text-dim text-meta mb-2">
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
                          <Link
                            href={`/cell/${output.txHash}-${output.outputIndex}`}
                            className="text-[color:var(--ink)]"
                            style={LINK}
                          >
                            out #{output.outputIndex}
                          </Link>
                          <span className="text-text">
                            {party ? (
                              <PartyLink party={party} />
                            ) : (
                              <span title={output.ownerLockHash}>
                                Other lock · {output.ownerLockHash.slice(0, 10)}…
                                {output.ownerLockHash.slice(-6)}
                              </span>
                            )}{' '}
                            · {ckb(output.capacity)}
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
