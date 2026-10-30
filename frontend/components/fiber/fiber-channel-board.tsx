'use client';

import Link from '@/components/ui/link';
import { TransactionEvidence } from '@/components/cell/cell-transactions';
import type { FiberChannelDetail, FiberChannelEvidence, FiberEpochDelay } from '@/lib/api';
import { ExactCkb } from '@/components/activity-event-row';
import { formatBlockNumber } from '@/lib/format-block';
import { u64Hex } from '@/lib/hex-bytes';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';

interface Props {
  channel: FiberChannelDetail;
  evidence?: FiberChannelEvidence;
  evidenceError?: string;
}
const short = (hash: string) => `${hash.slice(0, 10)}…${hash.slice(-8)}`;

/** A stage tx that is on chain has a block; a missing one is an API contract violation. */
function stageBlock(block: number | null, stage: string, txHash: string): number {
  if (block === null) throw new Error(`Fiber channel ${stage} tx ${txHash} has no block number`);
  return block;
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

function observedDuration(ms: string | null): string {
  if (ms === null) return '—';
  const value = BigInt(ms);
  if (value < BigInt(0)) throw new Error(`Negative Fiber lifetime ${ms}`);
  const days = value / BigInt(86_400_000);
  const hours = (value % BigInt(86_400_000)) / BigInt(3_600_000);
  const minutes = (value % BigInt(3_600_000)) / BigInt(60_000);
  return `${days} d ${hours} h ${minutes} m`;
}

export function FiberChannelBoard({ channel, evidence, evidenceError }: Props) {
  const phase =
    channel.state === 'open'
      ? 'Open on CKB'
      : channel.state === 'cooperativelyClosed'
        ? 'Cooperatively closed'
        : channel.state === 'forceClosed'
          ? 'Force-close waiting'
          : 'Settled';
  const tracked = evidence?.currentCommitment;
  const updatedCommitment = evidence?.stages.find((stage) => stage.stage === 'currentCommitment');
  if (updatedCommitment && (!tracked || updatedCommitment.txHash !== tracked.txHash)) {
    throw new Error('Fiber current commitment stage differs from tracked commitment cell');
  }
  const delayBytes = tracked
    ? u64Hex(tracked.delayEpoch, `Fiber commitment ${tracked.txHash} delay`)
    : null;
  return (
    <div>
      <div className="space-y-8">
        <div className="border-base-border grid min-h-[72px] gap-4 border-y py-3 sm:grid-cols-2 lg:grid-cols-4">
          <div>
            <div className="text-text-dim text-[11px]">Funding cell capacity</div>
            <div className="text-text-bright mt-1 font-mono text-[18px]">
              <ExactCkb shannons={BigInt(channel.capacity)} signed={false} tone="plain" />
            </div>
            <div className="text-text-dim text-[11px]">
              {channel.udtTypeHash
                ? 'Token-funded channel · CKB capacity shown'
                : 'CKB-funded channel'}
            </div>
          </div>
          <div>
            <div className="text-text-dim text-[11px]">On-chain state</div>
            <div className="text-jade mt-1 text-[18px]">{phase}</div>
            <div className="text-text-dim text-[11px]">
              {channel.state === 'forceClosed'
                ? 'tracked commitment remains live'
                : 'from canonical channel records'}
            </div>
          </div>
          <div>
            <div className="text-text-dim text-[11px]">Observed lifetime</div>
            <div className="text-text-bright mt-1 font-mono text-[18px]">
              {observedDuration(channel.observedLifetimeMs)}
            </div>
            <div className="text-text-dim text-[11px]">
              {channel.state === 'open' ? 'open → indexed tip' : 'open → close'} · block #
              {channel.lifetimeAnchorBlock ?? '—'}
            </div>
          </div>
          <div>
            <div className="text-text-dim text-[11px]">Observed parties</div>
            <div className="text-text-bright mt-1 font-mono text-[18px]">
              {channel.observedParties.length}
            </div>
            <div className="text-text-dim text-[11px]">
              non-Fiber locks seen on opening inputs or outputs
            </div>
          </div>
        </div>

        <section aria-label="Channel parties" className="border-base-border border-t pt-4">
          <h2 className="text-text-bright text-[18px]">Parties seen on chain</h2>
          <p className="text-text-dim mt-1 text-[12px]">
            These non-Fiber locks appeared on inputs or outputs of the opening transaction. An
            output-only lock did not necessarily sign it. They are not a live Fiber peer directory.
          </p>
          <div className="mt-3 grid gap-3 md:grid-cols-2">
            {channel.observedParties.map((party, index) => {
              const flow = evidence?.openingPartyFlows.find(
                (item) => item.lockScriptHash === party.lockScriptHash
              );
              return (
                <div
                  key={party.lockScriptHash}
                  className="border-base-border min-w-0 border-t py-2 text-[12px]"
                >
                  <span className="text-text-dim font-mono text-[11px]">PARTY {index + 1}</span>
                  <div className="mt-1 break-all font-mono">
                    {party.address ? (
                      <Link
                        href={`/address/${party.address}`}
                        className="text-jade hover:underline"
                      >
                        {party.address}
                      </Link>
                    ) : (
                      <span className="text-text-dim">
                        Address unavailable · {party.lockScriptHash}
                      </span>
                    )}
                  </div>
                  {flow && (
                    <div className="text-text-dim mt-2 space-y-1">
                      <div>
                        Opening inputs:{' '}
                        <span className="text-text-bright">
                          <ExactCkb
                            shannons={BigInt(flow.openingInputCapacity)}
                            signed={false}
                            tone="plain"
                          />
                        </span>
                      </div>
                      <div>
                        Outputs to same lock:{' '}
                        <span className="text-text-bright">
                          <ExactCkb
                            shannons={BigInt(flow.openingOutputCapacity)}
                            signed={false}
                            tone="plain"
                          />
                        </span>
                      </div>
                      <div>
                        Observed net input:{' '}
                        <span className="text-text-bright">
                          <ExactCkb shannons={BigInt(flow.observedNetInput)} tone="plain" />
                        </span>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
          {evidence && (
            <p className="text-text-dim mt-2 text-[11px]">
              Net input includes fees and any unrelated outputs in the same transaction; it is not a
              proven channel balance or payment.
            </p>
          )}
        </section>

        <section aria-label="On-chain stages" className="border-base-border border-t pt-4">
          <h2 className="text-text-bright text-[18px]">On-chain stages</h2>
          <p className="text-text-dim mt-1 text-[12px]">
            Opening, close, and any final settlement are separate CKB transactions. A partial
            settlement can replace the commitment cell before the final step.
          </p>
          <div className="mt-4 space-y-8">
            <div>
              <div className="mb-2 flex flex-wrap items-baseline gap-3">
                <span className="text-jade font-mono text-[12px]">01 OPENING</span>
                <Link
                  href={`/tx/${channel.fundingTxHash}`}
                  className="text-text-bright hover:text-jade break-all font-mono text-[12px]"
                >
                  {channel.fundingTxHash}
                </Link>
                <Link href={`/blocks/${channel.openBlock}`} className="text-text-dim text-[12px]">
                  block {formatBlockNumber(channel.openBlock)}
                </Link>
              </div>
              <TransactionEvidence
                hash={channel.fundingTxHash}
                label="Opening"
                focus={`out:${channel.fundingOutputIndex}`}
              />
            </div>
            {channel.closeTxHash ? (
              <div>
                <div className="mb-2 flex flex-wrap items-baseline gap-3">
                  <span className="text-text-bright font-mono text-[12px]">02 CLOSE</span>
                  <Link
                    href={`/tx/${channel.closeTxHash}`}
                    className="text-text-bright hover:text-jade break-all font-mono text-[12px]"
                  >
                    {channel.closeTxHash}
                  </Link>
                  <Link
                    href={`/blocks/${channel.closeBlock}`}
                    className="text-text-dim text-[12px]"
                  >
                    block{' '}
                    {formatBlockNumber(
                      stageBlock(channel.closeBlock, 'close', channel.closeTxHash)
                    )}
                  </Link>
                </div>
                <TransactionEvidence hash={channel.closeTxHash} label="Close" />
              </div>
            ) : (
              <div className="border-base-border text-text-dim border-t py-3 text-[12px]">
                02 Close has not appeared on chain.
              </div>
            )}
            {updatedCommitment && tracked && (
              <div>
                <div className="mb-2 flex flex-wrap items-baseline gap-3">
                  <span className="text-text-bright font-mono text-[12px]">
                    03 UPDATED COMMITMENT
                  </span>
                  <Link
                    href={`/tx/${updatedCommitment.txHash}`}
                    className="text-text-bright hover:text-jade break-all font-mono text-[12px]"
                  >
                    {updatedCommitment.txHash}
                  </Link>
                  <Link
                    href={`/blocks/${updatedCommitment.blockNumber}`}
                    className="text-text-dim text-[12px]"
                  >
                    block {formatBlockNumber(updatedCommitment.blockNumber)}
                  </Link>
                </div>
                <p className="text-text-dim mb-2 text-[11px]">
                  Latest tracked commitment after a partial settlement; earlier replacements are not
                  indexed as channel stages.
                </p>
                <TransactionEvidence
                  hash={updatedCommitment.txHash}
                  label="Updated commitment"
                  focus={`out:${tracked.outputIndex}`}
                />
              </div>
            )}
            {channel.settlementTxHash ? (
              <div>
                <div className="mb-2 flex flex-wrap items-baseline gap-3">
                  <span className="text-text-bright font-mono text-[12px]">
                    {updatedCommitment ? '04' : '03'} SETTLEMENT
                  </span>
                  <Link
                    href={`/tx/${channel.settlementTxHash}`}
                    className="text-text-bright hover:text-jade break-all font-mono text-[12px]"
                  >
                    {channel.settlementTxHash}
                  </Link>
                  <Link
                    href={`/blocks/${channel.settlementBlock}`}
                    className="text-text-dim text-[12px]"
                  >
                    block{' '}
                    {formatBlockNumber(
                      stageBlock(channel.settlementBlock, 'settlement', channel.settlementTxHash)
                    )}
                  </Link>
                </div>
                <TransactionEvidence hash={channel.settlementTxHash} label="Settlement" />
              </div>
            ) : (
              <div className="border-base-border text-text-dim border-t py-3 text-[12px]">
                {updatedCommitment ? '04' : '03'}{' '}
                {channel.state === 'forceClosed'
                  ? 'Final settlement has not appeared on chain.'
                  : 'No separate settlement transaction is recorded.'}
              </div>
            )}
          </div>
        </section>
      </div>
      <DetailTier2>
        <DetailSection title="Locks and commitment evidence">
          {evidenceError && (
            <p role="alert" className="text-rouge mt-3 break-all text-[12px]">
              Local node evidence unavailable: {evidenceError}
            </p>
          )}
          {evidence && (
            <div className="space-y-5 text-[12px]">
              <div className="grid gap-2 sm:grid-cols-[170px_minmax(0,1fr)]">
                <span className="text-text-dim">Funding lock · args</span>
                <span className="text-text-bright min-w-0 break-all font-mono">
                  {evidence.funding.lockArgs}
                </span>
                <span className="text-text-dim">Funding script</span>
                <Link
                  href={`/script/${evidence.funding.lockCodeHash}?hashType=${evidence.funding.lockHashType}`}
                  className="text-jade min-w-0 break-all font-mono"
                >
                  {evidence.funding.lockCodeHash}
                </Link>
              </div>
              {tracked ? (
                <div className="border-base-border grid gap-2 border-t pt-4 sm:grid-cols-[170px_minmax(0,1fr)]">
                  <span className="text-text-dim">Current commitment</span>
                  <Link
                    href={`/cell/${tracked.txHash}-${tracked.outputIndex}`}
                    className="text-jade min-w-0 break-all font-mono"
                  >
                    {tracked.txHash}:{tracked.outputIndex}
                  </Link>
                  <span className="text-text-dim">Lock args · {tracked.lockFormat}</span>
                  <span className="text-text-bright min-w-0 break-all font-mono">
                    {tracked.lockArgs}
                  </span>
                  <span className="text-text-dim">Pubkey hash</span>
                  <span className="text-text-bright break-all font-mono">{tracked.pubkeyHash}</span>
                  <span className="text-text-dim">Delay · 8 bytes, LE</span>
                  <span className="text-text-bright min-w-0">
                    <span className="break-all font-mono">
                      {delayBytes!.littleEndian} = {delayBytes!.value}
                    </span>{' '}
                    · {formatEpochDelay(tracked.delayEpochDecoded, tracked.delayEpoch)}
                  </span>
                  <span className="text-text-dim">Version · exact u64</span>
                  <span className="text-text-bright break-all font-mono">{tracked.version}</span>
                  <span className="text-text-dim">Settlement hash / flag</span>
                  <span className="text-text-bright break-all font-mono">
                    {tracked.settlementHash ?? 'Absent in legacy format'} ·{' '}
                    {tracked.settlementFlag ?? 'unknown'}
                  </span>
                  <span className="text-text-dim">Epoch lock</span>
                  <span className="text-text-bright">
                    {tracked.epochLock === 'firstSettlementDelay'
                      ? `First settlement delay ${tracked.epochLockMetAtTip ? 'met' : 'waiting'} at indexed tip ${tracked.tipEpoch}; ready epoch ${tracked.readyEpoch}`
                      : tracked.epochLock === 'subsequentSettlementNoReappliedDelay'
                        ? 'Subsequent settlement cell: original delay is not reapplied; other transaction conditions remain.'
                        : 'Legacy 36-byte form: deployed delay rule not asserted here.'}
                  </span>
                  <span className="text-text-dim">Commitment script</span>
                  <Link
                    href={`/script/${tracked.lockCodeHash}?hashType=${tracked.lockHashType}`}
                    className="text-jade min-w-0 break-all font-mono"
                  >
                    {tracked.lockCodeHash}
                  </Link>
                </div>
              ) : (
                <p className="text-text-dim">No commitment cell has appeared on chain.</p>
              )}
              {evidence.settlementOutputs.length > 0 && (
                <div className="border-base-border border-t pt-4">
                  <h3 className="text-text-bright text-[14px]">Final transaction outputs</h3>
                  <p className="text-text-dim mt-1 text-[11px]">
                    All final transaction outputs are shown. Matches to observed party locks are
                    marked; these are not an off-chain payment history.
                  </p>
                  {evidence.settlementOutputs.map((output) => (
                    <div
                      key={output.outputIndex}
                      className="border-base-border grid gap-1 border-b py-2 sm:grid-cols-[80px_minmax(0,1fr)_150px]"
                    >
                      <Link
                        href={`/cell/${output.txHash}-${output.outputIndex}`}
                        className="text-jade"
                      >
                        out #{output.outputIndex}
                      </Link>
                      <span className="text-text-dim break-all font-mono">
                        {output.observedParty ? 'Observed party · ' : 'Other lock · '}
                        {short(output.ownerLockHash)}
                      </span>
                      <span className="text-text-bright font-mono sm:text-right">
                        <ExactCkb shannons={BigInt(output.capacity)} signed={false} tone="plain" />
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </DetailSection>
      </DetailTier2>
    </div>
  );
}

export { observedDuration };
