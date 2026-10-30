'use client';
import { useState, useCallback, type ReactNode } from 'react';
import { useSearchParams, usePathname, useRouter } from '@/src/navigation';
import { parseActivityCursor } from '@/lib/asset-utils';
import { useQuery } from '@tanstack/react-query';
import { api, type DotCellItem, type ItemKind } from '@/lib/api';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { AssetFeed } from '@/components/asset/asset-feed';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import { CellVersions } from '@/components/asset/cell-versions';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { Hash } from '@/components/ui/hash';
import { TenureBar } from '@/components/ui/tenure-bar';
import { ExactCkb } from '@/components/activity-event-row';
import {
  IdentityName,
  IdentityPositioning,
  IdentityStatusBadge,
  identityNameParts,
  type IdentityStandard,
  type IdentityState,
} from '@/components/identity/identity-header';
import {
  IdentitySummary,
  identityFacts,
  type IdentityProtocol,
} from '@/components/identity/identity-overview';
import { IdentityRecords } from '@/components/identity/identity-records';
import { DotcellRoles } from '@/components/identity/dotcell-roles';
import { AmongDidCkb, DidDocumentReadout } from '@/components/identity/did-readout';
import {
  BitCellAccountNow,
  DidStored,
  DotbitAccountData,
  DotcellStored,
  identityDataTitle,
} from '@/components/identity/identity-evidence';
import { getIdentityCollectionHref } from '@/lib/detail-routes';
import { IDENTITY_ACTION_WORDS, type ItemFeedSubject } from '@/lib/feed-headlines';
import { identityTerm, termDays, type IdentityTerm } from '@/lib/identity-term';
import { formatUtc8Date } from '@/lib/utils';
import { readDidDocument, splitHandle } from '@/lib/did-document';
import { formatStandard } from '@/components/activity-event-row';

type IdentityKind = Extract<ItemKind, 'dotbit' | 'bit-cell' | 'did' | 'dotcell'>;
export interface CanonicalIdentityDetailProps {
  kind: IdentityKind;
  id: string;
  title: string;
  isLive: boolean;
  createdAtBlock: number;
  error: Error | null;
  /** `.cell`: the name's detail — its state, grace, records, sub-names and sale. */
  dotcell?: DotCellItem;
  /** Extra tier-1 sections after the standard's own (the `.cell` sale). */
  children?: ReactNode;
  /** Extra tier-2 sections (the `.cell` name relationships). */
  tier2Extra?: ReactNode;
}

const STANDARD: Record<IdentityKind, IdentityStandard> = {
  dotbit: 'dotbit',
  'bit-cell': 'bit_cell',
  did: 'did_ckb',
  dotcell: 'dotcell',
};

/** How this standard's feed rows are named: `.cell` by bare actions, `.bit` by `dotbit:` ones. */
const FEED_SUBJECT: Record<IdentityKind, ItemFeedSubject> = {
  dotcell: { kind: 'identity', protocolName: '.cell' },
  dotbit: { kind: 'identity', protocol: 'dotbit' },
  did: { kind: 'identity', actionWords: IDENTITY_ACTION_WORDS.did_ckb },
  'bit-cell': { kind: 'identity', actionWords: IDENTITY_ACTION_WORDS.bit_cell },
};

const ACTION_COLUMN: Record<IdentityKind, string> = {
  dotbit: '.bit action · its cell after',
  'bit-cell': 'Action · its cell after',
  did: 'Action · its cell after',
  dotcell: '.cell action · its cell after',
};

/** The title's parts: the name, and the standard's suffix set in rose (ID-C10, ID-D1). */
function titleParts(kind: IdentityKind, title: string, protocol: IdentityProtocol | undefined) {
  if (kind === 'did') {
    const last = protocol?.kind === 'did_ckb' ? protocol.versions.at(-1)!.decoded : undefined;
    const handle = last ? readDidDocument(last.document).handles[0] : undefined;
    if (handle) return { ...splitHandle(handle), full: handle };
    return { name: last?.did ?? title, suffix: '', full: last?.did ?? title };
  }
  const account =
    protocol?.kind === 'dotbit' || protocol?.kind === 'bit_cell'
      ? protocol.versions.at(-1)!.decoded.account
      : title;
  const parts = identityNameParts(account, kind === 'dotcell' ? '.cell' : '.bit');
  return { ...parts, full: account };
}

function identityState(
  kind: IdentityKind,
  isLive: boolean,
  term: IdentityTerm | null,
  now: number,
  dotcell: DotCellItem | undefined
): IdentityState | null {
  if (kind === 'dotcell') {
    if (!dotcell) throw new Error('.cell page without its name detail');
    return dotcell.state;
  }
  if (kind === 'did') return isLive ? 'live' : 'inactive';
  if (!isLive) return kind === 'dotbit' ? 'recycled' : 'inactive';
  if (!term) return null;
  return term.kind === 'term' && now >= term.expiresAt ? 'expired' : 'live';
}

/** "3 years paid · 1 at registration, 2 renewals" — the term bar's caption. */
function termNote(term: Extract<IdentityTerm, { kind: 'term' }>, kind: IdentityKind) {
  const n = term.renewals.length;
  const renewals = n ? `${n} renewal${n === 1 ? '' : 's'}` : 'not renewed';
  if (term.graceEnd !== undefined)
    return `${renewals} · then ${termDays(term.graceEnd, term.expiresAt)} days of grace, to ${formatUtc8Date(term.graceEnd)}`;
  return `paid ${kind === 'bit-cell' ? 'from minting' : 'at registration'} · ${renewals}`;
}

export function CanonicalIdentityDetail({
  kind,
  id,
  title,
  isLive,
  createdAtBlock,
  error,
  dotcell,
  children,
  tier2Extra,
}: CanonicalIdentityDetailProps) {
  const search = useSearchParams(),
    pathname = usePathname(),
    router = useRouter();
  const [now] = useState(() => Date.now());
  const feedSubject = FEED_SUBJECT[kind];
  const standard = STANDARD[kind];
  const [initialActivityCursor] = useState(() =>
    parseActivityCursor(search.get('activity_cursor'))
  );
  const cursorChanged = useCallback(
    (cursor: string | undefined) => {
      const params = new URLSearchParams(search.toString());
      if (cursor) params.set('activity_cursor', cursor);
      else params.delete('activity_cursor');
      if (params.toString() !== search.toString())
        router.replace(params.size ? `${pathname}?${params}` : pathname, { scroll: false });
    },
    [search, pathname, router]
  );
  const versions = useQuery({
    queryKey: ['item-versions', kind, id],
    queryFn: () => api.getItemCellVersions(kind, id),
  });
  const protocol = useQuery({
    queryKey: ['item-protocol', kind, id],
    queryFn: () => api.getItemProtocol(kind, id),
    retry: false,
  });
  const pds = useQuery({
    queryKey: ['did-pds'],
    queryFn: () => api.getDidPdsDistribution(),
    enabled: kind === 'did',
  });
  if (protocol.data && protocol.data.kind !== standard)
    throw new Error(`${id}: expected ${standard} protocol, got ${protocol.data.kind}`);
  const p = protocol.data as IdentityProtocol | undefined;
  const term = p ? identityTerm(p, { graceEndsAt: dotcell?.graceEndsAt }) : null;
  const state = identityState(kind, isLive, term, now, dotcell);
  const parts = titleParts(kind, title, p);
  const subject = parts.full;
  const current = versions.data?.data.at(-1);
  const held = current ? (current.isLive ? current.capacity : '0') : null,
    occupied = current ? (current.isLive ? current.occupiedCapacity : '0') : null;
  const collectionHref = getIdentityCollectionHref(standard, kind);
  const didDecoded = p?.kind === 'did_ckb' ? p.versions.at(-1)!.decoded : undefined;
  const records =
    p?.kind === 'dotbit'
      ? p.versions.at(-1)!.decoded.records.map((r) => ({
          key: `${r.type}.${r.key}`,
          valueUtf8: r.value,
          valueHex: r.valueHex,
          ttl: r.ttl,
          address: null,
        }))
      : p?.kind === 'dotcell'
        ? p.versions.at(-1)!.decoded.records.map((r) => ({
            ...r,
            address: r.decodedAddress?.address ?? null,
            addressLockHash: r.decodedAddress?.lockHash ?? null,
          }))
        : [];
  const termSection =
    term?.kind === 'term' ? (
      <TenureBar
        name={subject}
        periods={term.periods}
        graceEnd={term.graceEnd}
        now={now}
        note={termNote(term, kind)}
      />
    ) : term?.kind === 'unrecorded' ? (
      <p className="text-text-dim">Version {term.version} of its cell records no expiry.</p>
    ) : null;
  return (
    <AssetDetailShell
      title={<IdentityName name={parts.name} suffix={parts.suffix} />}
      kind="identities"
      railAfterHeader
      breadcrumb={[
        { label: 'Inventory', href: '/inventory/tokens' },
        { label: 'Identities', href: '/inventory/identities' },
        { label: formatStandard(standard), href: collectionHref },
      ]}
      status={
        <>
          {kind === 'bit-cell' && <span className="text-text-dim text-[12px]">.bit Cell</span>}
          {state && <IdentityStatusBadge state={state} />}
        </>
      }
      identity={
        kind === 'did' && didDecoded ? (
          <Hash hash={didDecoded.did} truncate={false} className="text-text text-[13px]" />
        ) : undefined
      }
      intro={
        <>
          <IdentityPositioning standard={standard} />
          {p && term && (
            <IdentitySummary protocol={p} term={term} now={now} isLive={isLive} subject={subject} />
          )}
        </>
      }
      facts={
        p && term
          ? identityFacts({ protocol: p, term, now, isLive, createdAtBlock, dotcell })
          : undefined
      }
      feed={
        <AssetFeed
          key={`${kind}:${id}`}
          queryKey={`${kind}:${id}`}
          scope="Its whole life"
          item={feedSubject}
          initialActivityCursor={initialActivityCursor}
          onActivityCursorChange={cursorChanged}
          loadActivities={(q) => api.getItemActivityFeed(kind, id, q)}
          loadTransactions={(q) => api.getItemTransactions(kind, id, q)}
          context={{
            label: ACTION_COLUMN[kind],
            render: (tx) => <ItemTransactionContext transaction={tx} item={feedSubject} />,
          }}
        />
      }
      tier2={
        p ? (
          <>
            <AssetReadState label="raw identity evidence" error={protocol.error} retained />
            <ItemEvidence protocol={p} title={identityDataTitle(standard)} />
            <div className="grid min-w-0 gap-x-14 gap-y-10 lg:grid-cols-2">
              {p.kind === 'dotbit' && <DotbitAccountData protocol={p} />}
              {p.kind === 'bit_cell' && <BitCellAccountNow protocol={p} />}
              {p.kind === 'did_ckb' && <DidStored protocol={p} />}
              {p.kind === 'dotcell' && dotcell && <DotcellStored protocol={p} item={dotcell} />}
              {tier2Extra}
            </div>
          </>
        ) : (
          <AssetReadState
            label="raw identity evidence"
            error={protocol.error}
            loading={protocol.isLoading}
          />
        )
      }
    >
      <AssetReadState label="identity" error={error} retained />
      {p ? (
        <AssetReadState label="identity document" error={protocol.error} retained />
      ) : (
        <AssetReadState
          label="identity document"
          error={protocol.error}
          loading={protocol.isLoading}
        />
      )}
      {p?.kind === 'did_ckb' && (
        <>
          <DidDocumentReadout protocol={p} />
          {pds.data ? (
            <AmongDidCkb
              distribution={pds.data}
              endpoints={p.versions.at(-1)!.decoded.pdsEndpoints}
              collectionHref={collectionHref}
            />
          ) : (
            <AssetReadState label="did:ckb servers" error={pds.error} loading={pds.isLoading} />
          )}
        </>
      )}
      {p?.kind === 'dotcell' && (
        <>
          <IdentityRecords
            subject={subject}
            records={records}
            parties={{
              owner: p.versions.at(-1)!.decoded.owner,
              manager: p.versions.at(-1)!.decoded.manager,
            }}
          />
          <DotcellRoles protocol={p} />
        </>
      )}
      {termSection}
      {p?.kind === 'dotbit' && records.length > 0 && (
        <IdentityRecords subject={subject} records={records} />
      )}
      {children}
      <section aria-label="CKB held in its cell" className="space-y-4">
        <div className="flex flex-wrap items-baseline justify-between gap-4">
          <h2 className="text-text-bright text-[15px] font-medium">CKB held in its cell</h2>
          <span className="text-text-dim text-[12px]">1 CKB per byte</span>
        </div>
        {held !== null && occupied !== null && current?.isLive && (
          <p>
            The cell holds{' '}
            <span className="text-text-bright">
              <ExactCkb shannons={BigInt(held)} signed={false} tone="plain" />
            </span>
            . <ExactCkb shannons={BigInt(occupied)} signed={false} tone="plain" /> pay for its
            bytes;{' '}
            {BigInt(held) === BigInt(occupied) ? (
              'none is free.'
            ) : (
              <>
                <ExactCkb shannons={BigInt(held) - BigInt(occupied)} signed={false} tone="plain" />{' '}
                are free.
              </>
            )}
          </p>
        )}
        {held !== null && occupied !== null && (
          <CapacityPartition
            subject={subject}
            total={held}
            occupied={occupied}
            free={(BigInt(held) - BigInt(occupied)).toString()}
          />
        )}
        {versions.data ? (
          <>
            <AssetReadState label="cell history" error={versions.error} retained />
            <CellVersions subject={subject} history={versions.data} />
          </>
        ) : (
          <AssetReadState
            label="cell history"
            error={versions.error}
            loading={versions.isLoading}
          />
        )}
      </section>
    </AssetDetailShell>
  );
}
