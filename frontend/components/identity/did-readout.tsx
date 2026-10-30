import type { DidPdsDistribution, ItemProtocol } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { readDidDocument } from '@/lib/did-document';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { formatUtc8Date } from '@/lib/utils';

type DidProtocol = Extract<ItemProtocol, { kind: 'did_ckb' }>;

function Row({
  term,
  field,
  children,
}: {
  term: string;
  field: string;
  children: React.ReactNode;
}) {
  return (
    <div className="border-base-border-subtle grid min-w-0 gap-1 border-t py-3 sm:grid-cols-[210px_minmax(0,1fr)] sm:gap-4">
      <dt className="text-text-dim">
        {term}
        <span className="text-text-ghost block text-[11px]">{field}</span>
      </dt>
      <dd className="flex min-w-0 flex-col gap-1">{children}</dd>
    </div>
  );
}

/**
 * The DID document, read (ID-D2): its handle with what it was before, its personal data
 * server, its signing key — each named with the document field it comes from. The document as
 * stored (CBOR) and the API's format number stay in tier 2 (ID-D4).
 */
export function DidDocumentReadout({ protocol }: { protocol: DidProtocol }) {
  const versions = protocol.versions;
  const n = versions.length;
  if (!n) throw new Error(`${protocol.itemId}: did:ckb has no versions`);
  const docs = versions.map((v) => readDidDocument(v.decoded.document));
  const current = docs[n - 1];
  // The handle's last change: walk back while earlier versions name the same handles.
  const same = (a: string[], b: string[]) => a.join('\n') === b.join('\n');
  let handleFrom = n - 1;
  while (handleFrom > 0 && same(docs[handleFrom - 1].handles, current.handles)) handleFrom--;
  const keyChanged = (name: string, key: string) => {
    let from = n - 1;
    while (from > 0 && docs[from - 1].signingKeys.some((k) => k.name === name && k.key === key))
      from--;
    return from;
  };
  return (
    <section aria-label="DID document" className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 className="text-text-bright text-[15px] font-medium">DID document</h2>
        <span className="text-text-dim text-[12px]">
          read from the cell · version {n} of {n}
        </span>
      </div>
      <dl className="border-base-border-subtle flex flex-col border-b">
        <Row term="Handle" field="alsoKnownAs">
          {current.handles.length ? (
            current.handles.map((h) => (
              <span key={h} className="text-text-bright text-[15px] [overflow-wrap:anywhere]">
                at://{h}
              </span>
            ))
          ) : (
            <span className="text-text">none</span>
          )}
          {handleFrom > 0 && (
            <span className="text-text-dim text-[12px]">
              was{' '}
              <span className="text-text">
                {docs[handleFrom - 1].handles.length
                  ? docs[handleFrom - 1].handles.map((h) => `at://${h}`).join(', ')
                  : 'no handle'}
              </span>{' '}
              until {formatUtc8Date(versions[handleFrom].cell.timestamp)}
            </span>
          )}
        </Row>
        {current.services.map((s) => (
          <Row
            key={s.name}
            term={s.name === 'atproto_pds' ? 'Personal data server' : s.name}
            field={`services.${s.name}`}
          >
            <a
              href={s.endpoint}
              rel="noreferrer"
              className="text-text-bright text-[15px] [overflow-wrap:anywhere] hover:underline"
            >
              {s.endpoint} ↗
            </a>
            <span className="text-text-dim text-[12px]">
              {s.type}
              {s.type === 'AtprotoPersonalDataServer'
                ? ' · where its posts and data are kept, off chain'
                : ''}
            </span>
          </Row>
        ))}
        {current.signingKeys.map((k) => {
          const from = keyChanged(k.name, k.key);
          return (
            <Row
              key={k.name}
              term={k.name === 'atproto' ? 'Signing key' : `Signing key · ${k.name}`}
              field={`verificationMethods.${k.name}`}
            >
              <span className="text-text-bright [overflow-wrap:anywhere]">{k.key}</span>
              <span className="text-text-dim text-[12px]">
                {from === 0
                  ? 'unchanged since creation'
                  : `set ${formatUtc8Date(versions[from].cell.timestamp)}`}
              </span>
            </Row>
          );
        })}
      </dl>
      {current.unreadable.length > 0 && (
        <p className="text-text-dim text-[12px]">
          Not read here: {current.unreadable.join(', ')} — the document as stored is in the raw
          evidence below.
        </p>
      )}
    </section>
  );
}

const RANK_OPACITY = [1, 0.55, 0.3];

/**
 * "Among did:ckb" (ID-D3): every live did:ckb by the personal data server its document names,
 * this identity's server marked. Counts are the API's, whole-scope and exact.
 */
export function AmongDidCkb({
  distribution,
  endpoints,
  collectionHref,
}: {
  distribution: DidPdsDistribution;
  /** This identity's own PDS endpoints (the API's `pdsEndpoints`). */
  endpoints: readonly string[];
  collectionHref: string;
}) {
  const total = distribution.totalLiveIdentities;
  const groups = [...distribution.groups].sort((a, b) => b.count - a.count);
  const segments = [
    ...groups.map((g, i) => ({
      key: g.endpoint,
      label: g.endpoint.replace(/^https?:\/\//, ''),
      count: g.count,
      color: CLASS_COLORS.identities,
      opacity: RANK_OPACITY[Math.min(i, RANK_OPACITY.length - 1)],
      mine: endpoints.includes(g.endpoint),
    })),
    ...(distribution.withoutPds > 0
      ? [
          {
            key: '(none)',
            label: 'none named',
            count: distribution.withoutPds,
            color: CLASS_COLORS.ckb,
            opacity: 1,
            mine: endpoints.length === 0,
          },
        ]
      : []),
  ];
  const named = segments.reduce((s, g) => s + g.count, 0);
  return (
    <section aria-label="Among did:ckb" className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 className="text-text-bright text-[15px] font-medium">Among did:ckb</h2>
        <AppLink href={collectionHref} className="text-[12px]">
          All {total.toLocaleString('en-US')} →
        </AppLink>
      </div>
      <p>
        {total.toLocaleString('en-US')} did:ckb identities live on CKB. By the personal data server
        their documents name:
      </p>
      <div
        role="img"
        aria-label={`did:ckb identities by personal data server: ${segments.map((s) => `${s.label} ${s.count}`).join(', ')}`}
        className="flex h-2.5 w-full gap-0.5 overflow-hidden rounded-[5px]"
      >
        {segments.map((s) => (
          <span
            key={s.key}
            className="min-w-1"
            style={{ flex: `${s.count} 0 0`, backgroundColor: s.color, opacity: s.opacity }}
          />
        ))}
      </div>
      <ul className="flex flex-wrap gap-x-6 gap-y-1 text-[12px]">
        {segments.map((s) => (
          <li key={s.key} className="flex items-center gap-1.5">
            <span
              aria-hidden="true"
              className="h-2 w-2 rounded-sm"
              style={{ backgroundColor: s.color, opacity: s.opacity }}
            />
            <span className={s.mine ? 'text-text-bright' : 'text-text'}>{s.label}</span>
            <span className="text-text-dim tabular-nums">{s.count.toLocaleString('en-US')}</span>
            {s.mine && <span className="text-text-dim">· this one</span>}
          </li>
        ))}
      </ul>
      {named !== total && (
        <p className="text-text-dim text-[12px]">
          An identity can name more than one server, so the servers add up to{' '}
          {named.toLocaleString('en-US')}.
        </p>
      )}
    </section>
  );
}
