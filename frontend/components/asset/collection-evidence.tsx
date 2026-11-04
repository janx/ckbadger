'use client';
import { useQuery } from '@tanstack/react-query';
import { api, type ClusterProtocol, type CollectionInsights } from '@/lib/api';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { CurrentPatternEvidence } from '@/components/asset/genetics';
import { DecodeCoverage } from '@/components/asset/collection-objects';
import { AssetReadState } from '@/components/asset/detail-shell';
import { AppLink } from '@/components/ui/app-link';
import { TintedJson } from '@/components/ui/tinted-json';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { withFieldMeanings } from '@/lib/protocol-field-meanings';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Date } from '@/lib/utils';
import { retryTransientOnly } from '@/lib/query-client';
import { hexBytes } from '@/lib/hex-bytes';

const short = (value: string, head = 10, tail = 6) =>
  value.length > head + tail + 1 ? `${value.slice(0, head)}…${value.slice(-tail)}` : value;

/**
 * "The cluster cell, byte by byte" (consistency C5, Main board tier 2): the current cluster cell's
 * data as a byte map with its fields' meanings, then where the cell is, who holds it and its type.
 */
export function ClusterCellEvidence({
  clusterId,
  current,
}: {
  clusterId: string;
  current: NonNullable<ClusterProtocol['current']>;
}) {
  const { cell } = current;
  const type = cell.type;
  return (
    <ProtocolEvidence
      title="The cluster cell, byte by byte"
      dataHex={current.dataHex}
      fields={withFieldMeanings('spore_cluster', current.fields)}
      decoded={current.decoded}
    >
      <dl
        aria-label="The cluster cell"
        className="text-small grid grid-cols-[88px_minmax(0,1fr)] gap-x-4 gap-y-1.5"
      >
        <dt className="text-text-dim">{cell.isLive ? 'live cell' : 'cell'}</dt>
        <dd className="text-text m-0 min-w-0 [overflow-wrap:anywhere]">
          <AppLink href={`/cell/${cell.txHash}-${cell.outputIndex}`} title={cell.txHash}>
            {short(cell.txHash, 8, 4)}
            <OutpointIndex index={cell.outputIndex} />
          </AppLink>{' '}
          · created in{' '}
          <AppLink href={`/blocks/${cell.blockNumber}`}>
            {formatBlockNumber(cell.blockNumber)}
          </AppLink>{' '}
          · {formatUtc8Date(cell.timestamp)}
        </dd>
        <dt className="text-text-dim">held by</dt>
        <dd className="text-text m-0 min-w-0 [overflow-wrap:anywhere]">
          <AppLink href={`/address/${cell.ownerAddress}`} title={cell.ownerAddress}>
            {short(cell.ownerAddress, 12, 6)}
          </AppLink>{' '}
          · {cell.lockName ?? 'Unregistered lock'}
        </dd>
        {type && (
          <>
            <dt className="text-text-dim">type</dt>
            <dd className="text-text m-0 min-w-0 [overflow-wrap:anywhere]">
              <AppLink
                href={getScriptDetailHref({
                  name: cell.typeName,
                  codeHash: type.codeHash,
                  hashType: type.hashType,
                  scriptKind: 'type',
                })}
              >
                {cell.typeName ?? short(type.codeHash)}
              </AppLink>
              {type.args.toLowerCase() === clusterId.toLowerCase()
                ? ' · the cluster id is this cell’s type args'
                : ` · type args ${type.args}`}
            </dd>
          </>
        )}
      </dl>
    </ProtocolEvidence>
  );
}

/**
 * The cluster's description as stored (Main board: "Description, as stored · pretty JSON"),
 * folded: pretty-printed and tinted when it is JSON, the exact text otherwise.
 */
export function StoredDescription({ description }: { description: string | null }) {
  if (description === null)
    return <p className="text-text-dim text-meta">No description bytes recorded.</p>;
  let pretty: string | null = null;
  try {
    pretty = JSON.stringify(JSON.parse(description), null, 2);
  } catch {
    // not JSON: shown exactly as stored
  }
  return (
    <details className="min-w-0">
      <summary className="text-text-dim text-small cursor-pointer">
        Description, as stored{pretty !== null ? ' · pretty JSON' : ''}
      </summary>
      {pretty !== null ? (
        <TintedJson text={pretty} className="mt-2" />
      ) : (
        <pre className="text-meta text-text-dim mt-2 max-w-full whitespace-pre-wrap break-all">
          {description}
        </pre>
      )}
    </details>
  );
}

/**
 * "How an object is read" for a cluster: the pattern from its insights, what the decoded objects
 * show, the newest object read through it (one gallery row), and the decode coverage line.
 */
export function ClusterPatternReading({
  clusterId,
  insights,
  description,
}: {
  clusterId: string;
  insights: CollectionInsights;
  /** The cluster's stored description (`undefined` while it loads), for a declared pattern. */
  description: string | null | undefined;
}) {
  const dob = insights.currentPattern.status === 'dob';
  const sample = useQuery({
    queryKey: ['cluster-pattern-sample', clusterId],
    queryFn: () => api.getCollectionGallery(clusterId, { limit: 1 }),
    enabled: dob,
    retry: retryTransientOnly,
  });
  const first = sample.data?.data[0];
  if (!dob)
    return (
      <div className="min-w-0 space-y-3">
        <DecodeCoverage genetics={insights.genetics} />
        <CurrentPatternEvidence pattern={insights.currentPattern} />
      </div>
    );
  return (
    <div className="min-w-0 space-y-3">
      <CurrentPatternEvidence
        pattern={insights.currentPattern}
        genetics={insights.genetics}
        description={description}
        sample={
          first
            ? {
                id: first.sporeId,
                href: `/objects/${first.sporeId}`,
                dnaHex: first.dnaHex,
                traits: first.traits,
                decodeStatus: first.decodeStatus,
              }
            : null
        }
        status={<DecodeCoverage genetics={insights.genetics} />}
      />
      <AssetReadState
        label="an object to read"
        error={sample.error}
        loading={sample.isLoading}
        retained={!!sample.data}
      />
    </div>
  );
}

type ObjectCollectionDetail = Awaited<ReturnType<typeof api.getObjectCollection>>;

/**
 * An m-NFT class in tier 2 (lane E, C5): its class id drawn byte by byte — the issuer's 20 bytes,
 * then the class index — with the class's issued / planned counts and renderer, and its issuer's
 * record; the issuer's info bytes folded, as JSON when they are JSON.
 */
export function MnftClassEvidence({
  classDetail,
  issuerDetail,
}: {
  classDetail: NonNullable<ObjectCollectionDetail['classDetail']>;
  issuerDetail?: ObjectCollectionDetail['issuerDetail'];
}) {
  const bytes = hexBytes(classDetail.classId, 'm-NFT class ID');
  if (bytes.length !== 24)
    throw new Error(
      `m-NFT class ID ${classDetail.classId}: expected 24 bytes, got ${bytes.length}`
    );
  const issuer = `0x${bytes.slice(0, 20).join('')}`;
  if (issuer !== classDetail.issuerId.toLowerCase())
    throw new Error(
      `m-NFT class ID ${classDetail.classId}: issuer bytes ${issuer} differ from issuer ${classDetail.issuerId}`
    );
  const index = bytes.slice(20, 24).join('');
  const info = issuerDetail?.infoHex ? issuerInfo(issuerDetail.infoHex) : null;
  return (
    <ProtocolEvidence
      title="The class id, byte by byte"
      dataHex={classDetail.classId}
      fields={[
        {
          name: 'Issuer ID',
          start: 0,
          end: 20,
          valueHex: issuer,
          meaning: 'The issuer that created the class.',
        },
        {
          name: `Class index · big-endian = ${Number.parseInt(index, 16)}`,
          start: 20,
          end: 24,
          valueHex: index,
          meaning: 'Which of the issuer’s classes, a big-endian number.',
        },
      ]}
    >
      <dl
        aria-label="The class"
        className="text-small grid grid-cols-[88px_minmax(0,1fr)] gap-x-4 gap-y-1.5"
      >
        <dt className="text-text-dim">issued</dt>
        <dd className="text-text m-0">
          {classDetail.issued.toLocaleString('en-US')} · planned{' '}
          {classDetail.total.toLocaleString('en-US')}
        </dd>
        <dt className="text-text-dim">renderer</dt>
        <dd className="text-text m-0 min-w-0 [overflow-wrap:anywhere]">
          {classDetail.renderer ?? 'not recorded'}
        </dd>
        {issuerDetail && (
          <>
            <dt className="text-text-dim">issuer</dt>
            <dd className="text-text m-0 min-w-0 [overflow-wrap:anywhere]">
              {issuerDetail.name ?? 'Unnamed issuer'} ·{' '}
              {issuerDetail.classCount.toLocaleString('en-US')} classes ·{' '}
              {issuerDetail.setCount.toLocaleString('en-US')} sets
            </dd>
          </>
        )}
      </dl>
      {issuerDetail?.infoHex && info && (
        <details className="min-w-0">
          <summary className="text-text-dim text-small cursor-pointer">
            Issuer info, as stored · {info.kind}
          </summary>
          {info.kind === 'pretty JSON' ? (
            <TintedJson text={info.text} className="mt-2" />
          ) : (
            <pre className="text-meta text-text-dim mt-2 max-w-full whitespace-pre-wrap break-all">
              {info.text}
            </pre>
          )}
        </details>
      )}
    </ProtocolEvidence>
  );
}

/** The issuer's info bytes: pretty JSON when they decode to JSON, else text, else the hex. */
function issuerInfo(infoHex: string): { kind: 'pretty JSON' | 'text' | 'hex'; text: string } {
  const bytes = hexBytes(infoHex, 'm-NFT issuer info');
  let text: string;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(
      Uint8Array.from(bytes, (b) => Number.parseInt(b, 16))
    );
  } catch {
    return { kind: 'hex', text: infoHex };
  }
  try {
    return { kind: 'pretty JSON', text: JSON.stringify(JSON.parse(text), null, 2) };
  } catch {
    return { kind: 'text', text };
  }
}
