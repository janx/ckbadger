'use client';
import { Fragment, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type Cell } from '@/lib/api';
import { cellInventoryItem, type CellInventoryItem } from '@/lib/cell-inventory';
import { identityTitleParts } from '@/lib/identity-title';
import { readErrorText } from '@/lib/read-error';
import { retryTransientOnly } from '@/lib/query-client';
import type { AssetClass } from '@/lib/design-tokens';
import type { CompositionTier } from '@/lib/asset-utils';
import { ClassMark } from '@/components/ui/class-mark';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { AppLink } from '@/components/ui/app-link';
import Link from '@/components/ui/link';
import { kindOf } from '@/components/transaction/transaction-cell-tiles';
import { formatCount } from '@/lib/format-count';
import { unitText } from '@/components/ui/units';
import { ShortId } from '@/components/ui/short-id';

/**
 * The line itself: the class mark, the statement, the item's page as the line's one bright link,
 * the composition chip where the item has one, and a read failure said in place.
 */
function Line({
  mark,
  lead,
  href,
  name,
  tail,
  tier,
  error,
  what,
}: {
  mark: AssetClass;
  lead: string;
  href: string;
  name: ReactNode;
  tail?: ReactNode;
  tier?: CompositionTier;
  error?: Error | null;
  what?: string;
}) {
  return (
    <p
      data-testid="cell-inventory"
      className="text-section text-text max-w-measure min-w-0 font-normal [overflow-wrap:anywhere]"
    >
      <ClassMark kind={mark} size={16} className="mr-2 align-[-1px]" />
      {lead}{' '}
      <AppLink look="key" href={href}>
        {name}
      </AppLink>
      {tail}
      {tier && (
        <>
          {' '}
          <CompositionBadge tier={tier} className="align-[3px]" />
        </>
      )}
      {error && (
        <span role="alert" className="text-rouge text-small">
          {' '}
          · could not read {what}: {readErrorText(error)}
        </span>
      )}
    </p>
  );
}

/**
 * One part of a token's name (its amount, or its unit) as an atomic box capped at the line: whole,
 * on a line of its own, when it fits there; wrapping inside itself (at its spaces, then anywhere)
 * only when it alone is wider. Neither part has a length bound (a unit is the token's own text; an
 * amount of a 0-decimal token can be any number of digits), and `whitespace-nowrap` would forbid
 * even the last-resort break, so the `anywhere` sits here, on each part (seals by kind, C8, C9:
 * "5,913.45087427 LP-UTXOSwap V1" is 296 px; beside the 88 px cell stone the line is 254 px at 390
 * and 224 px at 360). The default baseline sets the box's last line on the sentence's, and the
 * link's colour and glow reach it by inheritance. A short id (19 characters at most) stays
 * `whitespace-nowrap`: its length is bounded.
 */
function NamePart({ children }: { children: string }) {
  return <span className="inline-block max-w-full [overflow-wrap:anywhere]">{children}</span>;
}

/**
 * A token's amount and unit, degrading in order (C9): on one line when the pair fits; otherwise the
 * amount whole on one line and the unit whole on the next (the one space between the two parts is
 * the only break the pair offers, so the unit's own hyphen is never taken while the unit fits a
 * line); and only a part wider than the line by itself wraps inside. The pair is an atomic box
 * capped at the line too, so it never leaves its amount on the lead's line and its unit below.
 */
function TokenAmount({ amount, unit }: { amount: string; unit: string }) {
  return (
    <span className="inline-block max-w-full">
      <NamePart>{amount}</NamePart> <NamePart>{unit}</NamePart>
    </span>
  );
}

/** "Nervape Mask 0x7e3bc1bc12…06216539" with the cluster's name; "a Spore object 0x…" before it. */
function SporeLine({ item }: { item: Extract<CellInventoryItem, { kind: 'spore' }> }) {
  const object = useQuery({
    queryKey: ['spore', item.id],
    queryFn: () => api.getSporeObject(item.id),
  });
  const cluster = useQuery({
    queryKey: ['cluster', item.clusterId],
    queryFn: () => api.getSporeCluster(item.clusterId!),
    enabled: item.clusterId !== null,
  });
  if (object.data && object.data.sporeId !== item.id)
    throw new Error(`Spore object read for ${item.id} answered ${object.data.sporeId}`);
  if (object.data && object.data.clusterId !== item.clusterId)
    throw new Error(
      `Spore ${item.id}: the cell names cluster ${item.clusterId}, the object read ${object.data.clusterId}`
    );
  const name = cluster.data?.name;
  return (
    <Line
      mark="objects"
      lead={name ? 'This cell is' : 'This cell is a'}
      href={item.href}
      name={
        <>
          {name ?? 'Spore object'} <ShortId value={item.id} kind="hash" size="row" />
        </>
      }
      tier={object.data?.mediaProfile?.tier}
      error={object.error ?? cluster.error}
      what={object.error ? 'the object' : 'its cluster'}
    />
  );
}

function SporeClusterLine({
  item,
}: {
  item: Extract<CellInventoryItem, { kind: 'spore-cluster' }>;
}) {
  const cluster = useQuery({
    queryKey: ['cluster', item.id],
    queryFn: () => api.getSporeCluster(item.id),
  });
  return (
    <Line
      mark="objects"
      lead="This cell defines the cluster"
      href={item.href}
      name={cluster.data?.name ?? <ShortId value={item.id} kind="hash" size="row" />}
      error={cluster.error}
      what="the cluster"
    />
  );
}

function MnftLine({ item }: { item: Extract<CellInventoryItem, { kind: 'm-nft' }> }) {
  const detail = useQuery({
    queryKey: ['mnft-item-detail', item.id],
    queryFn: () => api.getMnftItemDetail(item.id),
  });
  if (detail.data && detail.data.nftId !== item.id)
    throw new Error(`m-NFT read for ${item.id} answered ${detail.data.nftId}`);
  const d = detail.data;
  return (
    <Line
      mark="objects"
      lead={d ? 'This cell is' : 'This cell is an'}
      href={item.href}
      name={
        d ? (
          `${d.class.name} #${d.tokenIndex}`
        ) : (
          <>
            m-NFT object <ShortId value={item.id} kind="hash" size="row" />
          </>
        )
      }
      tier={d?.composition?.tier}
      error={detail.error}
      what="the object"
    />
  );
}

function MnftClassLine({ item }: { item: Extract<CellInventoryItem, { kind: 'm-nft-class' }> }) {
  const collection = useQuery({
    queryKey: ['object-collection', item.id],
    queryFn: () => api.getObjectCollection(item.id),
  });
  return (
    <Line
      mark="objects"
      lead="This cell defines the m-NFT class"
      href={item.href}
      name={collection.data?.name ?? <ShortId value={item.id} kind="hash" size="row" />}
      error={collection.error}
      what="the class"
    />
  );
}

const IDENTITY_WORD = { dotbit: '.bit', 'bit-cell': '.bit Cell', did: 'did:ckb', dotcell: '.cell' };

/**
 * `.bit` and `.cell` are named by the cell's own data; `.bit Cell` and did:ckb as their pages
 * name them (`identityTitleParts` over the item's protocol read, the page's own query).
 */
function IdentityLine({ item }: { item: Extract<CellInventoryItem, { kind: 'identity' }> }) {
  const read = item.name === null;
  const protocol = useQuery({
    queryKey: ['item-protocol', item.standard, item.id],
    queryFn: () => api.getItemProtocol(item.standard, item.id),
    enabled: read,
    retry: retryTransientOnly,
  });
  const expected = item.standard === 'did' ? 'did_ckb' : 'bit_cell';
  if (read && protocol.data && protocol.data.kind !== expected)
    throw new Error(`${item.id}: expected ${expected} protocol, got ${protocol.data.kind}`);
  const p = protocol.data as Parameters<typeof identityTitleParts>[2];
  const name = item.name ?? (p ? identityTitleParts(item.standard, '', p).full : null);
  return (
    <Line
      mark="identities"
      lead={name ? 'This cell is' : 'This cell is a'}
      href={item.href}
      name={
        name ?? (
          <>
            {IDENTITY_WORD[item.standard]} identity{' '}
            <ShortId value={item.id} kind="hash" size="row" />
          </>
        )
      }
      // Every identity standard lives in CKB cells (IdentityPositioning says it on their pages).
      tier="pure_ckb"
      error={protocol.error}
      what="the identity"
    />
  );
}

/** The sentence's frame, the inventory line's own: its class mark, then its words. */
function KindSentence({ mark, children }: { mark: AssetClass; children: ReactNode }) {
  return (
    <p
      data-testid="cell-kind"
      className="text-section text-text max-w-measure min-w-0 font-normal [overflow-wrap:anywhere]"
    >
      <ClassMark kind={mark} size={16} className="mr-2 align-[-1px]" />
      {children}
    </p>
  );
}

/**
 * What a cell that is no inventory item is (lane P1, the plan: "one sentence of what the cell is
 * for every kind"; review chain-b cell-plain): by its scripts and its data, from what the page has
 * read — the code it holds for the scripts the API names (`codeCellOf`, its Type ID version), a
 * dep group's members (the API's list), the registry's name for its type (`typeName`: undefined
 * while the registry is read, null when it names none), or no type at all. Its capacity, its state
 * and the block that made it are the status strip's; its lock the facts'.
 */
function CellKindLine({ cell, typeName }: { cell: Cell; typeName: string | null | undefined }) {
  if (cell.codeCellOf?.length) {
    const version = cell.codeCell?.typeIdVersions.find(
      (item) => item.codeCellTxHash === cell.txHash && item.codeCellOutputIndex === cell.outputIndex
    );
    return (
      <KindSentence mark={kindOf(cell)}>
        This cell holds the code of{' '}
        {cell.codeCellOf.map((script, i) => (
          <Fragment key={`${script.codeHash}:${script.hashType}`}>
            {i > 0 && (i === cell.codeCellOf!.length - 1 ? ' and ' : ', ')}
            <Link look="inline" href={`/script/${script.codeHash}?hashType=${script.hashType}`}>
              {script.name}
            </Link>
          </Fragment>
        ))}
        {version && `, v${version.ordinal}`}
      </KindSentence>
    );
  }
  if (cell.isDepGroup) {
    if (!cell.depGroupItems)
      throw new Error(`Cell ${cell.txHash}:${cell.outputIndex} is a dep group without its members`);
    return (
      <KindSentence mark={kindOf(cell)}>
        This cell is a dep group of {unitText(formatCount(cell.depGroupItems.length, 'cell'))}
      </KindSentence>
    );
  }
  if (cell.type)
    return (
      <KindSentence mark={kindOf(cell)}>
        {typeName === undefined
          ? 'This cell carries a type script'
          : typeName === null
            ? 'This cell carries a type script the registry does not name'
            : `This cell carries the type script ${typeName}`}
      </KindSentence>
    );
  // a cell with no type script that holds data says so (lane P1, third round; review p-logic-chain
  // NOTE 3: a contract binary no registered script references read "holds plain CKB")
  return (
    <KindSentence mark={kindOf(cell)}>
      {cell.dataSize > 0 ? (
        <>
          This cell holds {unitText(formatCount(cell.dataSize, 'byte'))} of data, with no type
          script
        </>
      ) : (
        'This cell holds plain CKB, with no type script'
      )}
    </KindSentence>
  );
}

/**
 * Lane U (U6): the cell page hero's one line naming the inventory item the cell is — the first line
 * under the stone and the name plate (seals by kind, C10); a handled cell whose data does not name
 * its item says so in place, never a guess. Lane P1: a cell that is no inventory item (an unhandled
 * protocol, a plain cell, a code cell, a dep group) is said by its scripts (`CellKindLine`), so
 * every cell's page opens on one sentence of what it is.
 */
export function CellInventoryLine({
  cell,
  typeName,
}: {
  cell: Cell;
  /** The registry's name for the cell's type script: undefined while read, null for none. */
  typeName: string | null | undefined;
}) {
  let item: CellInventoryItem | null;
  try {
    item = cellInventoryItem(cell, 'row');
  } catch (error) {
    return (
      <p
        role="alert"
        data-testid="cell-inventory-error"
        className="text-rouge text-small max-w-measure"
      >
        {(error as Error).message}
      </p>
    );
  }
  if (!item) return <CellKindLine cell={cell} typeName={typeName} />;
  switch (item.kind) {
    case 'spore':
      return <SporeLine item={item} />;
    case 'spore-cluster':
      return <SporeClusterLine item={item} />;
    case 'm-nft':
      return <MnftLine item={item} />;
    case 'm-nft-class':
      return <MnftClassLine item={item} />;
    case 'identity':
      return <IdentityLine item={item} />;
    case 'token':
      return (
        <Line
          mark="tokens"
          lead="This cell holds"
          href={item.href}
          name={<TokenAmount amount={item.amount} unit={item.unit} />}
        />
      );
    case 'token-without-amount':
      return (
        <Line
          mark="tokens"
          lead="This cell holds no readable amount of"
          href={item.href}
          name={<NamePart>{item.unit}</NamePart>}
          tail={
            <span data-testid="cell-inventory-no-amount" className="text-text-dim text-small">
              {' '}
              · {item.reason}
            </span>
          }
        />
      );
    case 'dao':
      // lane P1 (said once): the cell's capacity is the status strip's, exact
      return (
        <Line
          mark="dao"
          lead="This cell is a"
          href={item.href}
          name={item.stage === 'deposit' ? 'Nervos DAO deposit' : 'Nervos DAO withdrawal request'}
        />
      );
  }
}
