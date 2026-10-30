'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { ExactCkb } from '@/components/activity-event-row';
import { AssetAmount } from '@/components/asset/inventory-table';
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
import { epochDelayWords, fiberDuration, stageEpoch } from '@/lib/fiber-format';
import { formatCount } from '@/lib/format-count';
import { u64Hex } from '@/lib/hex-bytes';
import { formatUtc8Date } from '@/lib/utils';

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
    <p
      data-testid="channel-summary"
      className="text-text mt-2 max-w-[900px] text-[15px] leading-relaxed"
    >
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
      description: forced
        ? `One side posted the commitment: the last state both had signed. It moves the funding cell to a commitment lock with a wait of ${delay}, so the other side can answer with a newer state.`
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
    <div className="grid min-w-0 grid-cols-1 gap-x-3 gap-y-1.5 text-[12px] sm:grid-cols-[150px_minmax(0,1fr)]">
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
    commitmentStage && `commitment at epoch ${stageEpoch(commitmentStage.epoch)}`,
    tracked?.readyEpoch && `earliest payout at epoch ${tracked.readyEpoch}`,
    settlement && `settled at epoch ${stageEpoch(settlement.epoch)}`,
  ].filter((part): part is string => Boolean(part));
  const steps = channelSteps(channel, evidence);
  const [first, second] = channel.observedParties;
  const argsBytes = (hex: string) => (hex.length - 2) / 2;
  const partyOf = (lockHash: string) =>
    channel.observedParties.find((party) => party.lockScriptHash === lockHash);
  return (
    <div>
      <div className="flex flex-col gap-11">
        <div>
          <div
            data-testid="channel-pair"
            className="flex flex-wrap items-baseline gap-x-7 gap-y-1.5 text-[13px]"
          >
            {channel.observedParties.length > 0 && (
              <span>
                {first && <PartyLink party={first} />}
                {second && (
                  <>
                    {' '}
                    <span className="text-[#4b5266]">↔</span> <PartyLink party={second} />
                  </>
                )}
              </span>
            )}
            <span>
              <span className="text-text-dim">capacity</span> {ckb(channel.capacity)}
              {channel.udtTypeHash && channel.udtAmount !== null && token && (
                <>
                  {' '}
                  <span className="text-text-dim">· holds</span>{' '}
                  <AssetAmount amount={channel.udtAmount} decimals={token.decimals} />{' '}
                  {token.symbol ?? 'tokens'}
                </>
              )}
            </span>
          </div>
          <Summary channel={channel} evidence={evidence} />
          {evidenceError && (
            <p role="alert" className="text-rouge mt-3 break-all text-[12px]">
              Local node evidence unavailable: {evidenceError}
            </p>
          )}
        </div>

        {atOpen && (
          <DetailSection title="What moved inside the channel">
            <p className="text-text-dim mb-3.5 max-w-[900px] text-[12.5px]">
              The chain never sees a Fiber payment, only the split when the channel opens and the
              split when it closes.
              {atClose &&
                ' Both splits include fees, and the chain does not say which side paid them, so the amount that changed sides is not shown.'}
            </p>
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
          </DetailSection>
        )}

        <DetailSection
          title={
            channel.state === 'open' || channel.state === 'forceClosed'
              ? `On chain, so far in ${STEP_WORDS[steps.length]} step${steps.length === 1 ? '' : 's'}`
              : `On chain, in ${STEP_WORDS[steps.length]} steps`
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

      {evidence && (
        <DetailTier2>
          <div className="grid min-w-0 items-start gap-x-14 gap-y-10 lg:grid-cols-2">
            <DetailSection title="The two locks">
              <div className="flex flex-col gap-3 text-[12px]">
                <div>
                  <div className="text-text-dim">
                    Fiber Funding Lock · args, {argsBytes(evidence.funding.lockArgs)} bytes
                  </div>
                  <div className="text-text-bright break-all">{evidence.funding.lockArgs}</div>
                  <div className="text-text-dim">
                    blake160 of the two parties’ aggregated public key
                  </div>
                  <Link
                    href={`/script/${evidence.funding.lockCodeHash}?hashType=${evidence.funding.lockHashType}`}
                    className="break-all text-[#6aa9c4]"
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
                        className="min-w-0 break-all text-[#6aa9c4]"
                      >
                        {tracked.txHash}:{tracked.outputIndex}
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
                      {tracked.lockFormat === 'full57' && (
                        <>
                          <span className="text-text-dim">Settlement hash / flag</span>
                          <span className="text-text-bright break-all">
                            {tracked.settlementHash ?? 'none'} · {tracked.settlementFlag ?? 'none'}
                          </span>
                        </>
                      )}
                      <span className="text-text-dim">Commitment script</span>
                      <Link
                        href={`/script/${tracked.lockCodeHash}?hashType=${tracked.lockHashType}`}
                        className="min-w-0 break-all text-[#6aa9c4]"
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
                          : 'Legacy 36-byte form: the deployed delay rule is not asserted here.'}
                    </span>
                  </>
                )}
              </Grid>
            </DetailSection>
          </div>
          {evidence.openingPartyFlows.length > 0 && (
            <DetailSection title="Opening transaction, per party">
              <p className="text-text-dim mb-2 text-[11.5px]">
                Non-Fiber locks on the opening transaction’s inputs or outputs. An output-only lock
                did not necessarily sign it. Net input includes fees and any unrelated outputs in
                the same transaction; it is not a proven channel balance or payment.
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
                        in {ckb(flow.openingInputCapacity)} · back {ckb(flow.openingOutputCapacity)}{' '}
                        · net {ckb(flow.observedNetInput, true)}
                      </span>
                    </span>
                  );
                })}
              </Grid>
            </DetailSection>
          )}
          {evidence.settlementOutputs.length > 0 && (
            <DetailSection title="Final transaction outputs">
              <p className="text-text-dim mb-2 text-[11.5px]">
                All final transaction outputs are shown, the payouts and any change alike.
              </p>
              <Grid>
                {evidence.settlementOutputs.map((output) => {
                  const party = output.observedParty ? partyOf(output.ownerLockHash) : undefined;
                  if (output.observedParty && !party)
                    throw new Error(
                      `Fiber settlement output ${output.txHash}:${output.outputIndex} names an unlisted party ${output.ownerLockHash}`
                    );
                  return (
                    <span key={output.outputIndex} className="contents">
                      <Link
                        href={`/cell/${output.txHash}-${output.outputIndex}`}
                        className="text-[#6aa9c4]"
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
        </DetailTier2>
      )}
    </div>
  );
}
