'use client';
import type { ItemProtocol, MnftProtocol, DotCellPartyRef } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { formatUtc8Date } from '@/lib/utils';
export interface IdentityDocumentProps {
  protocol: ItemProtocol;
}
function secondsDate(value: string) {
  if (!/^(0|[1-9]\d*)$/.test(value)) throw new Error(`Invalid protocol Unix seconds ${value}`);
  return formatUtc8Date((BigInt(value) * BigInt(1000)).toString());
}
export function NamedParty({ party, label }: { party: DotCellPartyRef; label: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-[11px]">{label}</dt>
      <dd className="mt-1 break-all">
        {party.lockHash ? (
          <AppLink href={`/address/${party.address ?? party.lockHash}`}>
            <Hash hash={party.address ?? party.lockHash} className="text-[11px]" copyable={false} />
          </AppLink>
        ) : (
          <>
            <Hash hash={party.hashPrefix} truncate={false} className="text-[11px]" />
            <span className="text-text-dim mt-1 block text-[11px]">
              Named lock-hash prefix · no uniquely resolved address
            </span>
          </>
        )}
        {party.scriptName && (
          <span className="text-text-dim block text-[11px]">{party.scriptName}</span>
        )}
      </dd>
    </div>
  );
}
export function IdentityDocument({ protocol }: IdentityDocumentProps) {
  if (protocol.kind === 'spore' || protocol.kind === 'm-nft')
    throw new Error(`${protocol.itemId}: not an identity protocol`);
  const last = protocol.versions.at(-1);
  if (!last) throw new Error(`${protocol.itemId}: empty identity protocol history`);
  const timeline = (
    <section aria-label="Identity term and document versions" className="space-y-4">
      <h2 className="text-text-bright text-[15px]">
        {protocol.kind === 'did_ckb' ? 'Document versions' : 'Registration & renewals'}
      </h2>
      <ol className="border-base-border space-y-4 border-l pl-5">
        {protocol.versions.map((v, i) => (
          <li key={`${v.cell.txHash}:${v.cell.outputIndex}`} className="min-w-0">
            <div className="flex flex-wrap items-baseline gap-3">
              <AppLink href={`/blocks/${v.cell.blockNumber}`}>
                Version {i + 1} · #{v.cell.blockNumber.toLocaleString('en-US')}
              </AppLink>
              <span className="text-text-dim text-[11px]">
                {formatUtc8Date(v.cell.timestamp)} UTC+8{v.cell.isLive ? ' · current cell' : ''}
              </span>
            </div>
            {'expiredAt' in v.decoded && (
              <p className="text-[11px]">
                Expires{' '}
                {v.decoded.expiredAt === null
                  ? 'not recorded'
                  : `${secondsDate(v.decoded.expiredAt)} UTC+8 · Unix ${v.decoded.expiredAt}`}
              </p>
            )}
            <AppLink href={`/tx/${v.cell.txHash}`} className="text-text-dim text-[11px]">
              Transaction <Hash hash={v.cell.txHash} copyable={false} className="text-[11px]" />
            </AppLink>
          </li>
        ))}
      </ol>
    </section>
  );
  if (protocol.kind === 'did_ckb') {
    const current = protocol.versions.at(-1)!.decoded;
    const document = current.document;
    const isPlainMap =
      document !== null &&
      typeof document === 'object' &&
      !Array.isArray(document) &&
      !Object.keys(document).some((key) =>
        ['$integer', '$tag', '$bytes', '$map', '$float'].includes(key)
      );
    return (
      <div className="space-y-10">
        <section aria-label="DID document" className="space-y-4">
          <h2 className="text-text-bright text-[15px]">DID document</h2>
          <div className="break-all">
            <Hash hash={current.did} truncate={false} className="text-[13px]" />
          </div>
          <p className="text-text-dim text-[11px]">
            No expiry. This identifier persists while its canonical cell is live. Document version{' '}
            {current.documentVersion} · local ID {current.localId ?? 'not set'}.
          </p>
          <div className="space-y-2">
            <h3 className="text-[13px]">Personal data servers</h3>
            {current.pdsEndpoints.length ? (
              current.pdsEndpoints.map((endpoint) => (
                <a
                  key={endpoint}
                  href={endpoint}
                  rel="noreferrer"
                  className="text-aqua block break-all text-[13px]"
                >
                  {endpoint} ↗
                </a>
              ))
            ) : (
              <p className="text-text-dim text-[11px]">No PDS endpoint recorded.</p>
            )}
          </div>
          {isPlainMap ? (
            <dl className="divide-base-border divide-y">
              {Object.entries(document as Record<string, unknown>).map(([key, value]) => (
                <div
                  key={key}
                  className="grid min-w-0 gap-2 py-3 sm:grid-cols-[140px_minmax(0,1fr)]"
                >
                  <dt className="text-text-dim text-[11px]">{key}</dt>
                  <dd>
                    <pre className="min-w-0 whitespace-pre-wrap break-all text-[11px]">
                      {typeof value === 'string' ? value : JSON.stringify(value, null, 2)}
                    </pre>
                  </dd>
                </div>
              ))}
            </dl>
          ) : (
            <pre className="whitespace-pre-wrap break-all text-[11px]">
              {typeof document === 'string' ? document : JSON.stringify(document, null, 2)}
            </pre>
          )}
          <p className="text-text-dim text-[11px]">
            Large integers, bytes and CBOR tags retain their typed representation; the exact CBOR is
            below.
          </p>
        </section>
        {timeline}
      </div>
    );
  }
  if (protocol.kind === 'dotbit') {
    const d = protocol.versions.at(-1)!.decoded;
    return (
      <div className="space-y-10">
        <section className="space-y-4" aria-label="Account rights">
          <h2 className="text-text-bright text-[15px]">{d.account}</h2>
          <dl className="grid gap-5 sm:grid-cols-2">
            {(['owner', 'manager'] as const).map((role) => (
              <div key={role}>
                <dt className="text-text-dim text-[11px]">
                  {role} · algorithm {d[role].algorithm}
                </dt>
                <dd className="mt-1 break-all">
                  <Hash hash={d[role].argsHex} truncate={false} className="text-[11px]" />
                </dd>
              </div>
            ))}
          </dl>
          <p className="text-text-dim text-[11px]">
            Registered {secondsDate(d.registeredAt)} UTC+8 · expires{' '}
            {d.expiredAt === null ? 'not recorded' : `${secondsDate(d.expiredAt)} UTC+8`} · account
            status {d.status} · witness version {d.witnessVersion}
          </p>
          {d.nextAccountId !== null && (
            <p className="text-[11px]">
              Next account{' '}
              <AppLink href={`/identities/dotbit/${d.nextAccountId}`}>
                {d.nextAccountName ?? d.nextAccountId}
              </AppLink>
            </p>
          )}
        </section>
        <section aria-label="Account records" className="space-y-4">
          <h2 className="text-text-bright text-[15px]">Records</h2>
          {d.records.length ? (
            <dl className="divide-base-border divide-y">
              {d.records.map((r, i) => (
                <div
                  key={`${r.type}:${r.key}:${i}`}
                  className="grid gap-2 py-3 sm:grid-cols-[140px_minmax(0,1fr)]"
                >
                  <dt className="text-text-dim break-words text-[11px]">
                    {r.label || r.key} · {r.type}
                  </dt>
                  <dd className="break-all">
                    {r.value ?? r.valueHex}
                    <span className="text-text-dim block text-[11px]">TTL {r.ttl}s</span>
                  </dd>
                </div>
              ))}
            </dl>
          ) : (
            <p className="text-text-dim">No records in this version.</p>
          )}
        </section>
        {timeline}
      </div>
    );
  }
  if (protocol.kind === 'bit_cell') {
    const d = protocol.versions.at(-1)!.decoded;
    return (
      <div className="space-y-10">
        <section aria-label="Linked .bit account" className="space-y-4">
          <h2 className="text-text-bright text-[15px]">{d.account}</h2>
          <p>Expires {secondsDate(d.expiredAt)} UTC+8</p>
          <p className="text-text-dim text-[11px]">
            Expiry is a protocol timestamp; a live cell can remain after that date.
          </p>
          <div className="break-all text-[11px]">
            Account ID <Hash hash={d.accountId} truncate={false} />
          </div>
          {d.linkedAccount ? (
            <>
              <AppLink href={`/identities/dotbit/${d.linkedAccount.id}`}>
                {d.linkedAccount.name ?? d.linkedAccount.id} →
              </AppLink>
              <p className="text-text-dim text-[11px]">
                Current linked account {d.linkedAccount.isLive ? 'live' : 'inactive'}
                {d.linkedAccount.registeredAt !== null
                  ? ` · registered ${secondsDate(d.linkedAccount.registeredAt)} UTC+8`
                  : ''}
                {d.linkedAccount.expiredAt !== null
                  ? ` · expires ${secondsDate(d.linkedAccount.expiredAt)} UTC+8`
                  : ''}
              </p>
            </>
          ) : (
            <p className="text-text-dim text-[11px]">Linked account not indexed.</p>
          )}
        </section>
        {timeline}
      </div>
    );
  }
  const d = protocol.versions.at(-1)!.decoded;
  return (
    <div className="space-y-10">
      <section aria-label="Named ownership" className="space-y-4">
        <h2 className="text-text-bright text-[15px]">{d.name}</h2>
        <dl className="grid gap-5 sm:grid-cols-2">
          <NamedParty party={d.owner} label="Named owner" />
          <NamedParty party={d.manager} label="Named manager" />
        </dl>
        <p className="text-text-dim text-[11px]">
          These protocol rights are separate from the physical cell's lock owner. Expires{' '}
          {secondsDate(d.expiredAt)} UTC+8 · layout version {d.layoutVersion}.
        </p>
      </section>
      <section aria-label="Name records" className="space-y-4">
        <h2 className="text-text-bright text-[15px]">Records</h2>
        {d.records.length ? (
          <dl className="divide-base-border divide-y">
            {d.records.map((r, i) => (
              <div
                key={`${r.key}:${i}`}
                className="grid gap-2 py-3 sm:grid-cols-[140px_minmax(0,1fr)]"
              >
                <dt className="text-text-dim break-words text-[11px]">
                  {r.label || r.key || 'record'}
                </dt>
                <dd className="min-w-0 break-all">
                  {r.decodedAddress ? (
                    <AppLink href={`/address/${r.decodedAddress.address}`}>
                      {r.decodedAddress.address}
                    </AppLink>
                  ) : (
                    (r.valueUtf8 ?? r.valueHex)
                  )}
                  <span className="text-text-dim block text-[11px]">TTL {r.ttl}s</span>
                </dd>
              </div>
            ))}
          </dl>
        ) : (
          <p className="text-text-dim">No records in this version.</p>
        )}
      </section>
      {timeline}
    </div>
  );
}
export interface MnftPermissionsProps {
  permissions: MnftProtocol['permissions'];
  configure: number;
  state?: { claimed: boolean; locked: boolean; value: number };
}
export function MnftPermissions({ permissions, configure, state }: MnftPermissionsProps) {
  const labels: Record<keyof MnftProtocol['permissions'], string> = {
    allowClaim: 'Claim',
    allowLock: 'Lock',
    allowExtension: 'Extension',
    allowCharacteristicUpdate: 'Update characteristic',
    allowTransferBeforeClaim: 'Transfer before claim',
    allowTransferAfterClaim: 'Transfer after claim',
    allowDestroyBeforeClaim: 'Destroy before claim',
    allowDestroyAfterClaim: 'Destroy after claim',
  };
  return (
    <section aria-label="mNFT permissions" className="space-y-4">
      <h2 className="text-text-bright text-[15px]">What its configuration permits</h2>
      <p className="text-text-dim text-[11px]">
        Configure 0x{configure.toString(16).padStart(2, '0')} · zero permission bits allow the
        operation.
      </p>
      <dl className="grid gap-3 sm:grid-cols-2">
        {Object.entries(labels).map(([key, label]) => (
          <div key={key} className="border-base-border flex justify-between gap-3 border-b py-2">
            <dt className="text-[11px]">{label}</dt>
            <dd
              className={
                permissions[key as keyof typeof permissions] ? 'text-jade' : 'text-text-dim'
              }
            >
              {permissions[key as keyof typeof permissions] ? 'Allowed' : 'Forbidden'}
            </dd>
          </div>
        ))}
      </dl>
      {state && (
        <p className="text-[11px]">
          State 0x{state.value.toString(16).padStart(2, '0')} ·{' '}
          {state.claimed ? 'claimed' : 'unclaimed'} · {state.locked ? 'locked' : 'unlocked'}.{' '}
          {state.locked
            ? 'The locked state prevents transfer even when its configure permission allows it.'
            : 'These are configuration permissions, not a guarantee that a proposed operation satisfies every script condition.'}
        </p>
      )}
    </section>
  );
}
