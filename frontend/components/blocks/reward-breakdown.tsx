import Link from '@/components/ui/link';
import { Address } from '@/components/ui/address';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import type { Block, BlockProposalFees } from '@/lib/api';
import { REWARD_COLORS } from '@/lib/design-tokens';
import { formatScaledInteger } from '@/lib/exact-integer';
import { parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';

type RewardBlock = Pick<
  Block,
  | 'number'
  | 'miningReward'
  | 'miningRewardTxHash'
  | 'rewardBreakdown'
  | 'rewardPaidAtBlock'
  | 'rewardFinalizesAt'
  | 'minerAddress'
  | 'totalFees'
  | 'transactionsCount'
>;

/** `0.0506`: an exact CKB amount inside a sentence, trailing zeros dropped. */
function plainCkb(shannons: bigint): string {
  const { integer, fraction } = splitCkbAmount(shannons);
  return `${integer}${fraction}`;
}

const heights = (numbers: readonly number[]) => {
  const names = numbers.map(formatBlockNumber);
  return names.length < 3
    ? names.join(' and ')
    : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
};

/** What the 40% proposal share is paid from, as `/blocks/{id}/proposal-fees` reports it (R2b-a 2). */
function proposalNote(fees: BlockProposalFees | undefined, error: Error | null): string {
  const rule =
    'of the fees of the transactions this block proposed first, once a later block commits them';
  if (error) return `${rule} (the fee basis could not be read: ${error.message})`;
  if (!fees) return rule;
  const window = `its window runs to ${formatBlockNumber(fees.commitWindow.to)}`;
  if (fees.transactionsCount === 0)
    return fees.complete
      ? `no transaction it proposed first was committed in its window, ${formatBlockNumber(fees.commitWindow.from)} to ${formatBlockNumber(fees.commitWindow.to)}`
      : `none of the transactions it proposed first is committed yet; ${window}`;
  const basis = `of ${plainCkb(parseShannons(fees.fees, `block ${fees.number} proposal fees`))} in fees of the ${formatCount(fees.transactionsCount, 'transaction')} it proposed, committed in ${heights(fees.committedIn)}`;
  return fees.complete ? basis : `${basis} so far; ${window}`;
}

/**
 * The Block board's reward: the exact total and its payee, one share bar, and each share with
 * what this block did to earn it. Every amount is the API's `rewardBreakdown` (the node's
 * `get_block_economic_state`); the parts must add up to `miningReward`.
 */
export function RewardBreakdown({
  block,
  proposalFees,
  proposalFeesError = null,
}: {
  block: RewardBlock;
  /** `/blocks/{id}/proposal-fees`; undefined while it is read. */
  proposalFees?: BlockProposalFees;
  proposalFeesError?: Error | null;
}) {
  const context = `Block ${block.number} reward`;
  if (!block.rewardBreakdown || block.miningReward === null) {
    return (
      <p className="text-text-dim text-xs">
        {block.number === 0 ? (
          'Genesis has no mining reward.'
        ) : (
          <>
            The reward is paid by the cellbase of{' '}
            {block.rewardFinalizesAt === null ? (
              'a later block'
            ) : (
              <Link href={`/blocks/${block.rewardFinalizesAt}`}>
                {formatBlockNumber(block.rewardFinalizesAt)}
              </Link>
            )}
            ; it is not indexed yet.
          </>
        )}
      </p>
    );
  }
  const total = parseShannons(block.miningReward, `${context} total`);
  const fees = parseShannons(block.totalFees, `${context} fees`);
  const parts = (
    [
      ['primary', 'Primary issuance', 'primary issuance'],
      ['secondary', 'Secondary issuance', 'secondary issuance'],
      ['committed', 'Committed fees, 60%', 'committed fees'],
      ['proposal', 'Proposal fees, 40%', 'proposal fees'],
    ] as const
  ).map(([key, label, spoken]) => ({
    key,
    label,
    spoken,
    value: parseShannons(block.rewardBreakdown![key], `${context} ${key}`),
  }));
  if (parts.reduce((sum, p) => sum + p.value, BigInt(0)) !== total)
    throw new Error(`${context}: the four shares do not add up to ${block.miningReward}`);
  if (block.transactionsCount < 1) throw new Error(`${context}: a block always holds its cellbase`);
  const secondaryIssuance = parseShannons(
    block.rewardBreakdown.secondaryIssuance,
    `${context} secondary issuance`
  );
  if (parts[1].value > secondaryIssuance)
    throw new Error(`${context}: the miners' share exceeds the block's secondary issuance`);
  if (proposalFees && proposalFees.number !== block.number)
    throw new Error(`${context}: proposal fees answered for block ${proposalFees.number}`);
  const committedTxs = block.transactionsCount - 1;
  const notes: Record<(typeof parts)[number]['key'], string> = {
    primary: 'the whole primary issuance of this block',
    secondary: `the miners' share of this block's ${plainCkb(secondaryIssuance)} secondary issuance; the rest goes to DAO depositors or is burnt`,
    committed: `of ${plainCkb(fees)} in fees of the ${formatCount(committedTxs, 'transaction')} committed here`,
    proposal: proposalNote(proposalFees, proposalFeesError),
  };
  const share = (value: bigint) =>
    total === BigInt(0) ? '0.00' : formatScaledInteger(value * BigInt(100), total, 2);
  return (
    <div className="space-y-3 text-xs">
      <div data-testid="reward-total" className="text-text-bright text-[15px] font-medium">
        <ExactCkb shannons={total} signed={false} tone="plain" />
        {block.minerAddress && (
          <>
            <span className="text-text-dim text-xs font-normal"> to </span>
            <Address address={block.minerAddress} className="text-[13px] font-normal" />
          </>
        )}
      </div>
      {total > BigInt(0) && (
        <div
          role="img"
          aria-label={`Reward shares: ${parts.map((p) => `${p.spoken} ${share(p.value)}%`).join(', ')}`}
          className="bg-base-surface flex h-2.5 gap-0.5 overflow-hidden rounded-[5px]"
        >
          {parts
            .filter((p) => p.value > BigInt(0))
            .map((p) => (
              <span
                key={p.key}
                data-reward-part={p.key}
                style={{
                  flexGrow: Number(p.value) / Number(total),
                  flexBasis: 0,
                  minWidth: '3px',
                  backgroundColor: REWARD_COLORS[p.key],
                }}
              />
            ))}
        </div>
      )}
      <dl className="grid grid-cols-[14px_minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-2 md:grid-cols-[14px_190px_150px_minmax(0,1fr)]">
        {parts.map((p) => (
          <div key={p.key} className="contents">
            <span
              aria-hidden="true"
              className="h-2 w-2 self-center rounded-sm"
              style={{ backgroundColor: REWARD_COLORS[p.key] }}
            />
            <dt>{p.label}</dt>
            <dd className="text-right">
              <ExactCkb shannons={p.value} signed={false} tone="plain" unit={false} />
            </dd>
            <dd className="text-text-dim col-span-3 col-start-2 md:col-span-1 md:col-start-auto">
              {notes[p.key]}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

/** "paid by the cellbase of #N · 0x…": the block whose cellbase pays this reward, and that tx. */
export function RewardPaidBy({ block }: { block: RewardBlock }) {
  if (block.rewardPaidAtBlock === null) return null;
  return (
    <span className="text-text-dim text-xs">
      paid by the cellbase of{' '}
      <Link href={`/blocks/${block.rewardPaidAtBlock}`}>
        {formatBlockNumber(block.rewardPaidAtBlock)}
      </Link>
      {block.miningRewardTxHash && (
        <>
          {' · '}
          <Link href={`/tx/${block.miningRewardTxHash}`}>
            <Hash hash={block.miningRewardTxHash} copyable={false} className="text-xs" />
          </Link>
        </>
      )}
    </span>
  );
}
