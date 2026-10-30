'use client';
import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type DotCellItem, type ItemProtocol, type ProtocolVersion } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { useDetailTier } from '@/components/ui/detail-layout';
import { AssetReadState } from '@/components/asset/detail-shell';
import { truncateRecordHex } from '@/components/asset/identity-document';
import type { IdentityStandard } from '@/components/identity/identity-header';
import { formatUtc8DateTime, cn } from '@/lib/utils';

type Of<K extends ItemProtocol['kind']> = Extract<ItemProtocol, { kind: K }>;

function byteLength(hex: string, what: string) {
  if (!hex.startsWith('0x') || hex.length % 2 !== 0) throw new Error(`${what}: invalid data hex`);
  return (hex.length - 2) / 2;
}

/** The data map's title (ID-C9): what the cell is and its size, not the protocol slug. */
export function identityDataTitle(standard: IdentityStandard) {
  return (version: ProtocolVersion<unknown>): string => {
    const bytes = byteLength(version.dataHex, `${version.cell.txHash}:${version.cell.outputIndex}`);
    switch (standard) {
      case 'dotbit':
        return `Account cell data · ${bytes} bytes`;
      case 'dotcell':
        return `Name cell data · layout v${(version.decoded as { layoutVersion: number }).layoutVersion} · ${bytes} bytes`;
      case 'bit_cell':
      case 'did_ckb':
        return `Cell data · ${bytes} bytes`;
    }
  };
}

/** A tier-2 section: its heading quiet inside tier 2 (FINAL_REVIEW, ID-C12), named for the outline. */
function Tier2Section({ title, children }: { title: string; children: ReactNode }) {
  const tier = useDetailTier();
  return (
    <section aria-label={title} className="min-w-0 space-y-3">
      <h2
        className={cn(
          'font-medium',
          tier === 2 ? 'text-text text-[13px]' : 'text-text-bright text-[15px]'
        )}
      >
        {title}
      </h2>
      {children}
    </section>
  );
}

function Rows({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="grid min-w-0 grid-cols-1 gap-x-3.5 gap-y-2 text-[12px] sm:grid-cols-[200px_minmax(0,1fr)]">
      {rows.map(([term, value]) => (
        <div key={term} className="contents">
          <dt className="text-text-dim">{term}</dt>
          <dd className="min-w-0 [overflow-wrap:anywhere]">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

const Dim = ({ children }: { children: ReactNode }) => (
  <span className="text-text-dim">{children}</span>
);

/**
 * `.bit` account internals, decoded from the witness (ID-B1 / ID-B2): the raw status byte, the
 * witness version and the owner / manager keys as the lock stores them. The witness's
 * `last_*_at` times and the sub-account switch are not decoded by the API yet.
 */
export function DotbitAccountData({ protocol }: { protocol: Of<'dotbit'> }) {
  const v = protocol.versions.at(-1);
  if (!v) throw new Error(`${protocol.itemId}: .bit has no versions`);
  const d = v.decoded;
  const same = d.owner.algorithm === d.manager.algorithm && d.owner.argsHex === d.manager.argsHex;
  return (
    <Tier2Section title="Account data, from the witness">
      <Rows
        rows={[
          ['registered_at', formatUtc8DateTime(Number(d.registeredAt) * 1000)],
          [
            'expired_at',
            d.expiredAt === null ? (
              <Dim>not recorded</Dim>
            ) : (
              formatUtc8DateTime(Number(d.expiredAt) * 1000)
            ),
          ],
          [
            'status',
            <>
              0x{d.status.toString(16).padStart(2, '0')} <Dim>· raw byte, as stored</Dim>
            </>,
          ],
          ['witness version', String(d.witnessVersion)],
          // R2b-b-17: rows only for fields the API serves; "0" is a time never set.
          ...witnessTimes(d),
          ['records', `${d.records.length} ${d.records.length === 1 ? 'entry' : 'entries'}`],
          ['owner', `algorithm ${d.owner.algorithm} · ${d.owner.argsHex}`],
          [
            'manager',
            <>
              algorithm {d.manager.algorithm} · {d.manager.argsHex}
              {same && <Dim> (the same)</Dim>}
            </>,
          ],
          ['account id', d.accountId],
          [
            'next',
            d.nextAccountId === null ? (
              <Dim>none</Dim>
            ) : (
              <AppLink href={`/identities/dotbit/${d.nextAccountId}`}>
                {d.nextAccountName ?? d.nextAccountId}
              </AppLink>
            ),
          ],
        ]}
      />
      <p className="text-text-ghost text-[11.5px]">
        Decoded from the account data in the witness of tx{' '}
        <AppLink href={`/tx/${v.cell.txHash}`}>
          <Hash hash={v.cell.txHash} copyable={false} className="text-[11.5px]" />
        </AppLink>
      </p>
    </Tier2Section>
  );
}

function witnessTime(value: string | null): ReactNode {
  if (value === null) return <Dim>not in this witness version</Dim>;
  if (value === '0') return <Dim>never</Dim>;
  return formatUtc8DateTime(Number(value) * 1000);
}

function witnessTimes(d: Of<'dotbit'>['versions'][number]['decoded']): [string, ReactNode][] {
  const rows: [string, ReactNode][] = [];
  if (typeof d.updatedAt === 'string') rows.push(['updated_at', witnessTime(d.updatedAt)]);
  for (const [key, value] of [
    ['last_transfer_account_at', d.lastTransferAccountAt],
    ['last_edit_manager_at', d.lastEditManagerAt],
    ['last_edit_records_at', d.lastEditRecordsAt],
  ] as const)
    if (value !== undefined) rows.push([key, witnessTime(value)]);
  if (d.enableSubAccount !== undefined)
    rows.push([
      'enable_sub_account',
      d.enableSubAccount === null ? (
        <Dim>not in this witness version</Dim>
      ) : (
        String(d.enableSubAccount)
      ),
    ]);
  if (d.renewSubAccountPrice !== undefined)
    rows.push([
      'renew_sub_account_price',
      d.renewSubAccountPrice === null ? (
        <Dim>not in this witness version</Dim>
      ) : (
        <>
          {BigInt(d.renewSubAccountPrice).toLocaleString('en-US')} <Dim>· raw</Dim>
        </>
      ),
    ]);
  return rows;
}

/** The `.bit` account a `.bit Cell` names, as it is now (ID-BC2: its state at minting is not served). */
export function BitCellAccountNow({ protocol }: { protocol: Of<'bit_cell'> }) {
  const d = protocol.versions.at(-1)?.decoded;
  if (!d) throw new Error(`${protocol.itemId}: .bit Cell has no versions`);
  const a = d.linkedAccount;
  return (
    <Tier2Section title="The .bit account it names, now">
      <Rows
        rows={[
          ['account', d.account],
          ['account id', d.accountId],
          ...(a
            ? ([
                [
                  'account now',
                  <>
                    <AppLink href={`/identities/dotbit/${a.id}`}>{a.name ?? a.id}</AppLink>
                    <Dim> · {a.isLive ? 'live' : 'not live'}</Dim>
                  </>,
                ],
                [
                  'registered',
                  a.registeredAt === null ? (
                    <Dim>not recorded</Dim>
                  ) : (
                    formatUtc8DateTime(Number(a.registeredAt) * 1000)
                  ),
                ],
                [
                  'expires',
                  a.expiredAt === null ? (
                    <Dim>not recorded</Dim>
                  ) : (
                    formatUtc8DateTime(Number(a.expiredAt) * 1000)
                  ),
                ],
              ] as [string, ReactNode][])
            : ([['account now', <Dim key="none">not indexed</Dim>]] as [string, ReactNode][])),
        ]}
      />
    </Tier2Section>
  );
}

/** The did:ckb document as stored (ID-D4): the DID, its local ID, the data layout and the CBOR. */
export function DidStored({ protocol }: { protocol: Of<'did_ckb'> }) {
  const d = protocol.versions.at(-1)?.decoded;
  if (!d) throw new Error(`${protocol.itemId}: did:ckb has no versions`);
  const bytes = byteLength(d.documentCborHex, `${protocol.itemId} document`);
  return (
    <Tier2Section title={`Document as stored · CBOR, ${bytes.toLocaleString('en-US')} bytes`}>
      <Rows
        rows={[
          ['DID', d.did],
          ['local ID', d.localId ?? <Dim>not set</Dim>],
          ['data layout', `V${d.documentVersion} (union variant 0)`],
        ]}
      />
      <pre className="bg-base-elevated text-text whitespace-pre-wrap rounded-md px-3.5 py-3 text-[11.5px] [overflow-wrap:anywhere]">
        {JSON.stringify(d.document, null, 2)}
      </pre>
    </Tier2Section>
  );
}

const ZERO_ID = /^0x0{40}$/;

/**
 * `.cell` evidence (ID-DC6 / ID-DC7): the records exactly as the witness stores them, and the
 * name cell's namespace, shared lock and ring successor — all out of tier 1.
 */
export function DotcellStored({ protocol, item }: { protocol: Of<'dotcell'>; item: DotCellItem }) {
  const v = protocol.versions.at(-1);
  if (!v) throw new Error(`${protocol.itemId}: .cell has no versions`);
  const d = v.decoded;
  const next = useQuery({
    queryKey: ['dotcell-item-detail', d.nextId],
    queryFn: () => api.getDotCellItemDetail(d.nextId),
    enabled: !ZERO_ID.test(d.nextId),
    retry: false,
  });
  return (
    <>
      <Tier2Section title="Records as stored · witness">
        {d.records.length ? (
          <div className="text-[12px]">
            {d.records.map((r, i) => (
              <div
                key={`${r.key}:${i}`}
                className="border-base-border-subtle grid min-w-0 grid-cols-[minmax(0,160px)_70px_minmax(0,1fr)] gap-3 border-t py-1.5"
              >
                <span className="[overflow-wrap:anywhere]">{r.key || '(empty key)'}</span>
                <span className="text-text-dim">ttl {r.ttl}</span>
                <span className="text-text min-w-0 [overflow-wrap:anywhere]">
                  {truncateRecordHex(r.valueHex)}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-text-dim text-[12px]">No records in this version.</p>
        )}
      </Tier2Section>
      <Tier2Section title="Name cell">
        <Rows
          rows={[
            [
              `Type · ${v.cell.typeName ?? 'not named'}`,
              <>
                {item.namespaceArgs} <Dim>· the namespace, the same for every name</Dim>
              </>,
            ],
            [
              `Lock · ${v.cell.lockName ?? 'not named'}`,
              <>
                <AppLink href={`/address/${v.cell.ownerAddress}`}>
                  <Hash hash={v.cell.ownerAddress} copyable={false} className="text-[12px]" />
                </AppLink>{' '}
                <Dim>
                  · names are held under this lock; ownership is the owner field in the data
                </Dim>
              </>,
            ],
            ['Name id', item.identityId],
            ['Records hash', d.recordsHash],
            [
              'Next in the ring',
              ZERO_ID.test(d.nextId) ? (
                <Dim>the ring&apos;s end</Dim>
              ) : next.data ? (
                <AppLink href={`/identities/dotcell/${next.data.identityId}`}>
                  {next.data.name}
                </AppLink>
              ) : (
                <>
                  <AppLink href={`/identities/dotcell/${d.nextId}`}>
                    <Hash hash={d.nextId} copyable={false} className="text-[12px]" />
                  </AppLink>
                  <AssetReadState label="its name" error={next.error} loading={next.isLoading} />
                </>
              ),
            ],
          ]}
        />
      </Tier2Section>
    </>
  );
}
