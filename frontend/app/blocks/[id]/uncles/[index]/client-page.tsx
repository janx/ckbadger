'use client';

import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { useParams } from '@/src/navigation';
import { NotFoundPage } from '@/components/not-found-page';
import { ReadFailure } from '@/components/read-failure';
import { Hash } from '@/components/ui/hash';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { DaoHeaderFields } from '@/components/blocks/dao-header-fields';
import { api, isNotFoundApiError, type BlockUncle } from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { decodeDaoHeader } from '@/lib/dao-header';
import { formatBlockNumber } from '@/lib/format-block';
import { formatEpochPosition } from '@/lib/epoch';
import { Utc8Age, Utc8DateTime } from '@/components/ui/utc8-date';
import { HeadId } from '@/components/ui/head-id';

const INDEX = /^(0|[1-9]\d*)$/;
/** What `INDEX` accepts, as the not-an-id page says it. */
const UNCLE_INDEX_RULE = 'an uncle is named by its index in the block: a whole number from 0.';

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
  if (!valid) return <NotFoundPage notAnId={{ kind: 'uncle', reason: UNCLE_INDEX_RULE }} />;
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
  if (!query.data && isBadRequestApiError(query.error))
    return <NotFoundPage notAnId={{ kind: 'uncle', reason: query.error.apiMessage }} />;
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        {/* awaited, not isLoading: a read paused offline or between retries is still awaited */}
        {isAwaited(query) && <p role="status">Loading uncle…</p>}
        {query.error && !query.data && <ReadFailure what="uncle" error={query.error} />}
        {query.error && query.data && (
          <p className="max-w-measure" role="alert">
            Unable to refresh uncle: {query.error.message}. Showing previously loaded data.
          </p>
        )}
        {query.data && <UncleEvidence uncle={query.data} id={id} index={index} />}
      </main>
    </div>
  );
}

const blockLink = (number: number) => (
  <Link look="inline" href={`/blocks/${number}`}>
    {formatBlockNumber(number)}
  </Link>
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
  // An uncle is never genesis (its number is at least 1), so the header-epoch rule refuses the
  // zero epoch here.
  const epoch = formatEpochPosition(
    { number: uncle.epochNumber, index: uncle.epochIndex, length: uncle.epochLength },
    uncle.number
  );
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
      // The block's name plate (lane D2b; U-03, PA-06, the UI review's R8): the kind line says
      // "Uncle", so the h1 is the height alone, as the block page's is
      title={formatBlockNumber(uncle.number)}
      documentTitle={[formatBlockNumber(uncle.number), 'uncle']}
      identity={<HeadId value={uncle.hash} kind="hash" copy="copy uncle hash" />}
      // The block stone without rods, in the dead ink (the owner's ruling of 2026-11-19): a block
      // stone counts its transactions in rods, and the chain keeps none for an uncle (its header
      // and its proposals); an uncle is no longer in force, as a consumed cell is not
      seal={{
        id: { kind: 'block', hash: uncle.hash, txs: 'not kept' },
        state: 'ink',
        kind: 'Uncle',
      }}
      // The status strip under the id line (lane D2; PA-13, PA-06): the block that included it
      // (the dot in the dead ink: an uncle is no longer in force), its time with its age and the
      // delay to its sibling, the canonical block at its height and its epoch; its sentence
      // follows as the lede, where every entity page's does (the facts stood before it).
      strip={{
        label: 'Uncle status',
        facts: [
          {
            label: 'included by',
            value: (
              <Link look="inline" href={`/blocks/${uncle.includedBy.number}`}>
                {formatBlockNumber(uncle.includedBy.number)}
              </Link>
            ),
            dot: 'text-dim',
            note: `uncle ${index}`,
          },
          {
            label: 'time',
            value: <Utc8DateTime timestamp={uncle.timestamp} />,
            note: [
              <Utc8Age key="age" timestamp={uncle.timestamp} />,
              delay && `${delay} its sibling`,
            ],
          },
          {
            label: 'canonical',
            value: (
              <Link look="inline" href={`/blocks/${uncle.canonicalAtHeight.hash}`}>
                <Hash hash={uncle.canonicalAtHeight.hash} size="row" />
              </Link>
            ),
          },
          { label: 'epoch', value: epoch },
        ],
      }}
      lede={
        <>
          <p className="text-body leading-relaxed">
            Uncle at height {formatBlockNumber(uncle.number)}, included by{' '}
            {blockLink(uncle.includedBy.number)}; {formatBlockNumber(uncle.number)} on the canonical
            chain is{' '}
            <Link look="inline" href={`/blocks/${uncle.canonicalAtHeight.hash}`}>
              <Hash hash={uncle.canonicalAtHeight.hash} size="row" />
            </Link>
            {delay ? (
              <>
                , its sibling: both were mined on parent{' '}
                <Link look="inline" href={`/blocks/${uncle.parentHash}`}>
                  {formatBlockNumber(uncle.number - 1)} <Hash hash={uncle.parentHash} size="row" />
                </Link>
                , and this uncle came {delay} it.
              </>
            ) : (
              <>
                , which has another parent; this uncle was mined on parent{' '}
                <Link look="inline" href={`/blocks/${uncle.parentHash}`}>
                  <Hash hash={uncle.parentHash} size="row" />
                </Link>
                .
              </>
            )}{' '}
            It is not part of the canonical chain: its transactions never ran, but the proposals it
            carried count toward the proposal window.
          </p>
          {/* why its stone has no rods, said once (lane D2b) */}
          <p className="text-body mt-2 leading-relaxed">
            Its seal has no rods: a block’s seal counts its transactions in rods, and the chain
            keeps none for an uncle.
          </p>
        </>
      }
      tier2={
        <>
          <DetailSection title={`Proposals it carried (${uncle.proposalIds.length})`}>
            {uncle.proposalIds.length ? (
              <ul className="text-small space-y-1 font-mono">
                {uncle.proposalIds.map((proposal) => (
                  <li key={proposal} className="break-all">
                    {proposal}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-text-dim text-small">This uncle proposed nothing.</p>
            )}
          </DetailSection>
          <DetailSection title="Header">
            <dl className="text-small grid min-w-0 gap-3">
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
                    {key === 'parentHash' ? (
                      <Link look="inline" href={`/blocks/${value}`}>
                        {value}
                      </Link>
                    ) : (
                      value
                    )}
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
      {/* its first content is the head's strip and sentence; its evidence is tier 2 */}
      {null}
    </DetailLayout>
  );
}
