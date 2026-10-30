import type { ReactNode } from 'react';
import type { DotCellItem, ItemProtocol } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Address } from '@/components/ui/address';
import { ExactCkb } from '@/components/activity-event-row';
import { NamedParty } from '@/components/asset/identity-document';
import { ownerRun, termDays, type IdentityTerm } from '@/lib/identity-term';
import { describeIdentityRecords } from '@/lib/identity-records';
import { readDidDocument } from '@/lib/did-document';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';

export type IdentityProtocol = Exclude<ItemProtocol, { kind: 'spore' } | { kind: 'm-nft' }>;

function Sub({ children }: { children: ReactNode }) {
  return <span className="text-text-dim mt-0.5 block text-[12px] font-normal">{children}</span>;
}
function B({ children }: { children: ReactNode }) {
  return <span className="text-text-bright">{children}</span>;
}
const times = (n: number) => (n === 1 ? 'once' : n === 2 ? 'twice' : `${n} times`);
const plural = (n: number, one: string, many: string) =>
  `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;
function listJoin(items: string[]) {
  return items.length <= 1
    ? items.join('')
    : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

function currentCell(protocol: IdentityProtocol) {
  const last = protocol.versions.at(-1);
  if (!last) throw new Error(`${protocol.itemId}: identity has no protocol versions`);
  return last.cell;
}

function heldFact(protocol: IdentityProtocol, isLive: boolean) {
  const cell = currentCell(protocol);
  const held = isLive ? BigInt(cell.capacity) : BigInt(0);
  const occupied = isLive ? BigInt(cell.occupiedCapacity) : BigInt(0);
  if (occupied > held) throw new Error(`${protocol.itemId}: occupied exceeds the cell's capacity`);
  return {
    label: 'CKB held in its cell',
    value: (
      <>
        <ExactCkb shannons={held} signed={false} tone="plain" wrap />
        <Sub>
          {isLive ? (
            <>
              <span className="text-jade">
                <ExactCkb shannons={occupied} signed={false} tone="plain" />
              </span>{' '}
              occupied · <ExactCkb shannons={held - occupied} signed={false} tone="plain" /> free
            </>
          ) : (
            'no live cell'
          )}
        </Sub>
      </>
    ),
  };
}

function expiryFact(term: IdentityTerm, now: number, grace?: number) {
  if (term.kind === 'unrecorded')
    return {
      label: 'Expires',
      value: (
        <>
          not recorded<Sub>version {term.version} stores no expiry</Sub>
        </>
      ),
    };
  if (term.kind !== 'term') throw new Error('expiry fact of an identity without a term');
  const expired = now >= term.expiresAt,
    days = termDays(term.expiresAt, now),
    last = term.renewals.at(-1);
  return {
    label: expired ? 'Expired' : 'Expires',
    value: (
      <>
        {formatUtc8Date(term.expiresAt)}
        <Sub>
          {expired ? `${plural(days, 'day', 'days')} ago` : `in ${plural(days, 'day', 'days')}`}
          {last ? ` · renewed ${formatUtc8Date(last.at)}` : ' · never renewed'}
          {grace !== undefined ? ` · grace to ${formatUtc8Date(grace)}` : ''}
        </Sub>
      </>
    ),
  };
}

function blockFact(label: string, timestamp: string | number, block: number) {
  return {
    label,
    value: (
      <>
        {formatUtc8DateTime(timestamp)}
        <Sub>
          <AppLink href={`/blocks/${block}`}>{formatBlockNumber(block)}</AppLink>
        </Sub>
      </>
    ),
  };
}

function ownerFact(protocol: IdentityProtocol, note: string) {
  const cell = currentCell(protocol);
  return {
    label: 'Owner',
    value: (
      <>
        <Address address={cell.ownerAddress} className="text-[13px]" />
        <Sub>
          {cell.lockName ?? 'lock not named'}
          {note}
        </Sub>
      </>
    ),
  };
}

function physicalSince(protocol: IdentityProtocol, first: string) {
  const run = ownerRun(
    protocol.versions.map((v) => v.cell),
    (c) => c.ownerLockHash
  );
  return run.changes === 0
    ? ` · since ${first}`
    : ` · since ${formatUtc8Date(run.since.timestamp)}`;
}

export interface IdentityFactsInput {
  protocol: IdentityProtocol;
  term: IdentityTerm;
  now: number;
  isLive: boolean;
  createdAtBlock: number;
  /** `.cell`: the name's detail (records, sub-names, sale, grace). */
  dotcell?: DotCellItem;
}

/** The facts rail of an identity, in its own domain's words (ID-C6, IdentityBit/BitCell/Did/Dotcell). */
export function identityFacts({
  protocol,
  term,
  now,
  isLive,
  createdAtBlock,
  dotcell,
}: IdentityFactsInput): { label: string; value: ReactNode }[] {
  switch (protocol.kind) {
    case 'dotbit': {
      const d = protocol.versions.at(-1)!.decoded;
      const sameKey =
        d.owner.algorithm === d.manager.algorithm && d.owner.argsHex === d.manager.argsHex;
      return [
        ownerFact(
          protocol,
          sameKey
            ? ' · owner and manager are the same key'
            : ' · owner and manager are different keys'
        ),
        expiryFact(term, now),
        blockFact('Registered', Number(d.registeredAt) * 1000, createdAtBlock),
        {
          label: 'Records',
          value: d.records.length ? d.records.length.toLocaleString('en-US') : 'none set',
        },
        heldFact(protocol, isLive),
      ];
    }
    case 'bit_cell': {
      const d = protocol.versions.at(-1)!.decoded,
        mint = protocol.versions[0].cell;
      return [
        ownerFact(protocol, physicalSince(protocol, 'minting')),
        expiryFact(term, now),
        blockFact('Minted', mint.timestamp, mint.blockNumber),
        {
          label: '.bit account',
          value: d.linkedAccount ? (
            <>
              <AppLink href={`/identities/dotbit/${d.linkedAccount.id}`}>
                {d.linkedAccount.name ?? d.linkedAccount.id}
              </AppLink>
              <span className="text-text-dim font-normal">
                {' '}
                · {d.linkedAccount.isLive ? 'live' : 'not live'}
              </span>
            </>
          ) : (
            <span className="text-text-dim">not indexed</span>
          ),
        },
        heldFact(protocol, isLive),
      ];
    }
    case 'did_ckb': {
      const first = protocol.versions[0].cell,
        last = protocol.versions.at(-1)!.cell;
      return [
        ownerFact(protocol, physicalSince(protocol, 'creation')),
        blockFact('Created', first.timestamp, first.blockNumber),
        ...(protocol.versions.length > 1
          ? [blockFact('Last updated', last.timestamp, last.blockNumber)]
          : []),
        { label: 'Expiry', value: <span className="text-text">none</span> },
        heldFact(protocol, isLive),
      ];
    }
    case 'dotcell': {
      if (!dotcell) throw new Error(`${protocol.itemId}: .cell facts need the name's detail`);
      const stored = protocol.versions.at(-1)!.decoded.records;
      const records = describeIdentityRecords(
        stored.map((r) => ({ ...r, address: r.decodedAddress?.address ?? null }))
      );
      const first = protocol.versions[0].cell;
      const children = dotcell.children.length;
      return [
        {
          label: 'Owner',
          value: (
            <dl>
              <NamedParty party={dotcell.owner} label="named in the name's data" />
            </dl>
          ),
        },
        {
          label: 'Manager',
          value: (
            <dl>
              <NamedParty party={dotcell.manager} label="edits its records" />
            </dl>
          ),
        },
        expiryFact(term, now, term.kind === 'term' ? term.graceEnd : undefined),
        blockFact('Registered', first.timestamp, createdAtBlock),
        {
          label: 'Records',
          value: (
            <>
              {stored.length.toLocaleString('en-US')}
              <Sub>
                {plural(records.addresses.length, 'address', 'addresses')} ·{' '}
                {plural(records.profile.length, 'profile key', 'profile keys')}
              </Sub>
            </>
          ),
        },
        {
          label: 'Sub-names · for sale',
          value: (
            <span className="text-text">
              {children === 0 && !dotcell.childrenHasMore
                ? 'none'
                : dotcell.childrenHasMore
                  ? `${children.toLocaleString('en-US')} and more`
                  : children.toLocaleString('en-US')}{' '}
              · {dotcell.sale ? 'yes' : 'no'}
            </span>
          ),
        },
        heldFact(protocol, isLive),
      ];
    }
  }
}

function lockPhrase(name: string | null) {
  if (!name) return 'an unnamed lock';
  return name.toLowerCase().endsWith('lock') ? `the ${name}` : `a ${name} lock`;
}

/** The one-paragraph summary under an identity's title (ID-C5), every clause an API fact. */
export function IdentitySummary({
  protocol,
  term,
  now,
  isLive,
  subject,
}: {
  protocol: IdentityProtocol;
  term: IdentityTerm;
  now: number;
  isLive: boolean;
  subject: string;
}) {
  const termClause = (lead: ReactNode) => {
    if (term.kind !== 'term') return null;
    const days = plural(termDays(term.expiresAt, now), 'day', 'days');
    return now >= term.expiresAt ? (
      <>
        {lead}its term ended on <B>{formatUtc8Date(term.expiresAt)}</B>, <B>{days}</B> ago
      </>
    ) : (
      <>
        {lead}held until <B>{formatUtc8Date(term.expiresAt)}</B>, <B>{days}</B> from now
      </>
    );
  };
  let body: ReactNode;
  switch (protocol.kind) {
    case 'dotbit': {
      const d = protocol.versions.at(-1)!.decoded;
      const renewals = term.kind === 'term' ? term.renewals.length : 0;
      const owners = ownerRun(
        protocol.versions,
        (v) => `${v.decoded.owner.algorithm}:${v.decoded.owner.argsHex}`
      );
      body = (
        <>
          Registered on <B>{formatUtc8Date(Number(d.registeredAt) * 1000)}</B>
          {renewals ? ` and renewed ${times(renewals)}` : ''}
          {!isLive ? (
            '; it is no longer a live account. '
          ) : term.kind === 'term' ? (
            <>, {termClause(now >= term.expiresAt ? '' : 'it is ')}. </>
          ) : (
            '. Its expiry is not recorded. '
          )}
          {owners.changes === 0
            ? 'Its owner has never changed. '
            : `Its owner changed ${times(owners.changes)}, most recently on ${formatUtc8Date(owners.since.cell.timestamp)}. `}
          {d.records.length
            ? `${plural(d.records.length, 'record is', 'records are')} set.`
            : 'No records are set.'}
        </>
      );
      break;
    }
    case 'bit_cell': {
      const d = protocol.versions.at(-1)!.decoded,
        mint = protocol.versions[0].cell;
      const owners = ownerRun(
        protocol.versions.map((v) => v.cell),
        (c) => c.ownerLockHash
      );
      const wallet = mint.lockName
        ? `a ${mint.lockName} wallet`
        : 'a wallet whose lock is not named';
      body = (
        <>
          Minted on <B>{formatUtc8Date(mint.timestamp)}</B> into <B>{wallet}</B>
          {!isLive
            ? '; the cell is no longer live. '
            : owners.changes === 0
              ? ' that still holds it. '
              : `; since ${formatUtc8Date(owners.since.timestamp)} ${lockPhrase(owners.since.lockName)} holds it. `}
          {term.kind === 'term' && (
            <>
              {now >= term.expiresAt ? (
                <>
                  The name&apos;s {termClause('')}
                  {d.linkedAccount && !d.linkedAccount.isLive
                    ? '; the .bit account is no longer live, but this cell remains.'
                    : '.'}
                </>
              ) : (
                <>It is {termClause('')}.</>
              )}
            </>
          )}
        </>
      );
      break;
    }
    case 'did_ckb': {
      const versions = protocol.versions;
      const last = versions.at(-1)!;
      const doc = readDidDocument(last.decoded.document);
      const pds = last.decoded.pdsEndpoints;
      const names = [
        ...(pds.length === 1
          ? [`a personal data server at ${pds[0].replace(/^https?:\/\//, '')}`]
          : pds.length > 1
            ? [`${pds.length} personal data servers`]
            : []),
        ...(doc.signingKeys.length
          ? [
              doc.signingKeys.length === 1
                ? 'one signing key'
                : `${doc.signingKeys.length} signing keys`,
            ]
          : []),
      ];
      body = (
        <>
          A Web5 identity, created on <B>{formatUtc8Date(versions[0].cell.timestamp)}</B>.
          {versions.length > 1 && (
            <>
              {' '}
              Its document was updated {times(versions.length - 1)}, most recently on{' '}
              <B>{formatUtc8Date(last.cell.timestamp)}</B>.
            </>
          )}
          {names.length > 0 && <> It names {listJoin(names)}.</>}
          {isLive ? ' It has no expiry date.' : ' It is no longer live.'}
        </>
      );
      break;
    }
    case 'dotcell': {
      const versions = protocol.versions;
      const d = versions.at(-1)!.decoded;
      const owners = ownerRun(versions, (v) => v.decoded.owner.hashPrefix);
      const coins = describeIdentityRecords(
        d.records.map((r) => ({ ...r, address: r.decodedAddress?.address ?? null }))
      ).addresses.map((a) => a.coin);
      body = (
        <>
          Registered on <B>{formatUtc8Date(versions[0].cell.timestamp)}</B>
          {term.kind === 'term' ? <> and {termClause('')}. </> : '. '}
          {owners.changes === 0 ? (
            'Its owner has never changed. '
          ) : (
            <>
              It changed hands {times(owners.changes)}; since{' '}
              {formatUtc8Date(owners.since.cell.timestamp)} <B>{lockPhrase(d.owner.scriptName)}</B>{' '}
              owns it
              {d.manager.hashPrefix !== d.owner.hashPrefix
                ? ` while ${lockPhrase(d.manager.scriptName)} manages its records`
                : ''}
              .{' '}
            </>
          )}
          {coins.length === 0
            ? 'It points to no address.'
            : coins.length === 1
              ? `It points to a ${coins[0]} address.`
              : `It points to ${listJoin(coins)} addresses.`}
        </>
      );
      break;
    }
  }
  return (
    <p
      data-testid="identity-summary"
      aria-label={`${subject} in brief`}
      className="text-text mt-1.5 max-w-[820px] text-[15px] leading-relaxed"
    >
      {body}
    </p>
  );
}
