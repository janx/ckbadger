'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { notFound, useParams } from '@/src/navigation';
import { Header } from '@/components/layout/header';
import { NotFoundPage } from '@/components/not-found-page';
import { Hash } from '@/components/ui/hash';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { DaoHeaderFields } from '@/components/blocks/dao-header-fields';
import { api, isNotFoundApiError, type BlockUncle } from '@/lib/api';
import { decodeDaoHeader } from '@/lib/dao-header';
import { formatTimeAgo, formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatEpochPosition } from '@/lib/epoch';

const INDEX = /^(0|[1-9]\d*)$/;

export default function UncleDetailPage() {
  const params = useParams();
  const id = String(params.id);
  const rawIndex = String(params.index);
  const valid = INDEX.test(rawIndex) && Number.isSafeInteger(Number(rawIndex));
  const index = Number(rawIndex);
  const query = useQuery({
    queryKey: ['block-uncle', id, index],
    queryFn: () => api.getBlockUncle(id, index),
    enabled: valid,
  });
  if (!valid) notFound();
  if (!query.data && isNotFoundApiError(query.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'uncle',
          identifier: `${id} · uncle ${index}`,
          checkedEndpoint: `/blocks/${encodeURIComponent(id)}/uncles/${index}`,
        }}
      />
    );
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        {query.isLoading && <p role="status">Loading uncle…</p>}
        {query.error && (
          <p role="alert">
            Unable to {query.data ? 'refresh' : 'read'} uncle: {query.error.message}
            {query.data ? '. Showing previously loaded data.' : ''}
          </p>
        )}
        {query.data && <UncleEvidence uncle={query.data} id={id} index={index} />}
      </main>
    </div>
  );
}

const blockLink = (number: number) => (
  <Link href={`/blocks/${number}`}>{formatBlockNumber(number)}</Link>
);

function UncleEvidence({ uncle, id, index }: { uncle: BlockUncle; id: string; index: number }) {
  const context = `Uncle ${index} of block ${formatBlockNumber(uncle.includedBy.number)}`;
  const requested = /^\d+$/.test(id)
    ? uncle.includedBy.number === Number(id)
    : uncle.includedBy.hash.toLowerCase() === id.toLowerCase();
  if (!requested) throw new Error(`${context}: the API answered for another including block`);
  if (uncle.includedBy.index !== index)
    throw new Error(`${context}: the API answered for uncle ${uncle.includedBy.index}`);
  if (uncle.canonicalAtHeight.number !== uncle.number)
    throw new Error(
      `${context}: canonical block ${formatBlockNumber(uncle.canonicalAtHeight.number)} is not at its height`
    );
  if (uncle.canonicalAtHeight.hash.toLowerCase() === uncle.hash.toLowerCase())
    throw new Error(`${context}: the uncle hash is the canonical block at its height`);
  // R10: the API says whether the canonical block at this height shares the uncle's parent.
  if ((uncle.sameParentAs === null) !== (uncle.msAfterSibling === null))
    throw new Error(`${context}: sibling facts disagree (sameParentAs vs msAfterSibling)`);
  if (uncle.sameParentAs !== null && uncle.sameParentAs !== uncle.canonicalAtHeight.number)
    throw new Error(
      `${context}: sibling ${formatBlockNumber(uncle.sameParentAs)} is not the canonical block at its height`
    );
  const delay =
    uncle.msAfterSibling === null
      ? null
      : uncle.msAfterSibling >= 0
        ? `${uncle.msAfterSibling.toLocaleString('en-US')} ms after`
        : `${(-uncle.msAfterSibling).toLocaleString('en-US')} ms before`;
  const dao = decodeDaoHeader(uncle.dao.raw, context);
  const epoch = formatEpochPosition({
    number: uncle.epochNumber,
    index: uncle.epochIndex,
    length: uncle.epochLength,
  });
  const decoded = { c: dao.C, ar: dao.AR, s: dao.S, u: dao.U };
  for (const [field, value] of Object.entries(decoded))
    if (uncle.dao[field as keyof typeof decoded] !== value.toString())
      throw new Error(`${context}: DAO ${field.toUpperCase()} disagrees with the raw header bytes`);
  const header: [string, string][] = [
    ['parentHash', uncle.parentHash],
    ['compactTarget', uncle.compactTarget],
    ['nonce', uncle.nonce],
    ['transactionsRoot', uncle.transactionsRoot],
    ['proposalsHash', uncle.proposalsHash],
    ['extraHash', uncle.extraHash],
  ];
  return (
    <DetailLayout
      title={`Uncle ${formatBlockNumber(uncle.number)}`}
      identity={<Hash hash={uncle.hash} truncate={false} />}
      facts={[
        {
          label: 'Included by',
          value: (
            <>
              {blockLink(uncle.includedBy.number)}
              <span className="text-text-dim"> · uncle {index}</span>
            </>
          ),
        },
        {
          label: `Canonical ${formatBlockNumber(uncle.number)}`,
          value: (
            <Link href={`/blocks/${uncle.canonicalAtHeight.hash}`}>
              <Hash hash={uncle.canonicalAtHeight.hash} copyable={false} />
            </Link>
          ),
        },
        {
          label: 'Time',
          value: (
            <>
              {formatUtc8DateTime(uncle.timestamp)}
              <span className="text-text-dim block text-[11px]">
                {formatTimeAgo(uncle.timestamp)}
              </span>
              {delay && (
                <span className="text-text-dim block text-[11px]">{delay} its sibling</span>
              )}
            </>
          ),
        },
        {
          label: 'Epoch',
          value: epoch,
        },
      ]}
      tier2={
        <>
          <DetailSection title={`Proposals it carried (${uncle.proposalIds.length})`}>
            {uncle.proposalIds.length ? (
              <ul className="space-y-1 font-mono text-xs">
                {uncle.proposalIds.map((proposal) => (
                  <li key={proposal} className="break-all">
                    {proposal}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-text-dim text-xs">This uncle proposed nothing.</p>
            )}
          </DetailSection>
          <DetailSection title="Header">
            <dl className="grid min-w-0 gap-3 text-xs">
              <div className="text-text-dim">Header version {uncle.version}</div>
              <div className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                <dt className="text-text-dim">difficulty</dt>
                <dd className="break-all">{BigInt(uncle.difficulty).toLocaleString('en-US')}</dd>
              </div>
              <div className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                <dt className="text-text-dim">epoch</dt>
                {/* R2b-a 6 (BA-41): the packed header epoch, read as number · index / length. */}
                <dd>
                  <span className="break-all">{uncle.rawEpoch}</span>
                  <span className="text-text-dim"> = </span>
                  <span>{epoch}</span>
                </dd>
              </div>
              {header.map(([key, value]) => (
                <div key={key} className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                  <dt className="text-text-dim">{key}</dt>
                  <dd className="break-all">
                    {key === 'parentHash' ? <Link href={`/blocks/${value}`}>{value}</Link> : value}
                  </dd>
                </div>
              ))}
              <div className="grid gap-1 sm:grid-cols-[160px_minmax(0,1fr)]">
                <dt className="text-text-dim">dao</dt>
                <dd className="min-w-0">
                  <span className="break-all">{uncle.dao.raw}</span>
                  <DaoHeaderFields dao={dao} />
                </dd>
              </div>
            </dl>
          </DetailSection>
        </>
      }
    >
      <p className="text-[13px] leading-relaxed">
        Uncle at height {formatBlockNumber(uncle.number)}, included by{' '}
        {blockLink(uncle.includedBy.number)}; {formatBlockNumber(uncle.number)} on the canonical
        chain is{' '}
        <Link href={`/blocks/${uncle.canonicalAtHeight.hash}`}>
          <Hash hash={uncle.canonicalAtHeight.hash} copyable={false} />
        </Link>
        {delay ? (
          <>
            , its sibling: both were mined on parent{' '}
            <Link href={`/blocks/${uncle.parentHash}`}>
              {formatBlockNumber(uncle.number - 1)}{' '}
              <Hash hash={uncle.parentHash} copyable={false} />
            </Link>
            , and this uncle came {delay} it.
          </>
        ) : (
          <>
            , which has another parent; this uncle was mined on parent{' '}
            <Link href={`/blocks/${uncle.parentHash}`}>
              <Hash hash={uncle.parentHash} copyable={false} />
            </Link>
            .
          </>
        )}{' '}
        It is not part of the canonical chain: its transactions never ran, but the proposals it
        carried count toward the proposal window.
      </p>
    </DetailLayout>
  );
}
